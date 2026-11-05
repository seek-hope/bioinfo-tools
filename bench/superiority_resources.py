#!/usr/bin/env python3
"""Linux sampled process-tree/file guard for expanded performance experiments.

This records observations, not an OS resource reservation or hard limit. RSS is
summed across visible processes (shared pages can be counted more than once).
Short-lived, reparented or session-escaping descendants can evade discovery.
"""
from __future__ import annotations
from dataclasses import asdict, dataclass
import math
import os
from pathlib import Path
import signal
import stat
import threading
import time

GIB = 1024**3


@dataclass(frozen=True)
class Limits:
    rss_bytes: int = 8 * GIB
    files_bytes: int = 32 * GIB
    headroom_bytes: int = 16 * GIB
    seconds: float = 1800.0
    interval_seconds: float = 1.0

    def __post_init__(self):
        for field in ('rss_bytes', 'files_bytes', 'headroom_bytes'):
            value = getattr(self, field)
            if type(value) is not int or value <= 0:
                raise ValueError(f'{field} must be a positive integer')
        for field in ('seconds', 'interval_seconds'):
            value = getattr(self, field)
            if isinstance(value, bool) or not isinstance(value, (int, float)) or not math.isfinite(value) or value <= 0:
                raise ValueError(f'{field} must be finite and positive')
        if self.interval_seconds > self.seconds:
            raise ValueError('sampling interval exceeds timeout')


def parse_process(pid: int, text: str) -> dict:
    # Linux comm may contain spaces, newlines and closing parentheses.
    closing = text.rfind(')')
    if closing < 0 or int(text.split(' ', 1)[0]) != pid:
        raise ValueError('invalid proc stat identity')
    fields = text[closing + 1:].split()
    return {'pid': pid, 'state': fields[0], 'ppid': int(fields[1]), 'pgrp': int(fields[2]),
            'session': int(fields[3]), 'start_ticks': int(fields[19]),
            'rss_bytes': max(0, int(fields[21])) * os.sysconf('SC_PAGE_SIZE')}


def processes(leader: int, known: dict[int, int]) -> dict:
    """Discover the session and visible descendants, retaining PID/start identity."""
    rows = {}
    vanished = unreadable = 0
    with os.scandir('/proc') as entries:
        for entry in entries:
            if not entry.name.isdecimal():
                continue
            pid = int(entry.name)
            try:
                text = Path(entry.path, 'stat').read_text(errors='surrogateescape')
                rows[pid] = parse_process(pid, text)
            except (FileNotFoundError, ProcessLookupError):
                vanished += 1
            except PermissionError:
                # Unreadable system processes need not belong to this invocation.
                # A previously discovered process becoming unreadable is fatal.
                if pid == leader or pid in known:
                    raise
                unreadable += 1
    session_matches = leader not in rows or leader not in known or rows[leader]['start_ticks'] == known[leader]
    selected = {pid for pid, row in rows.items()
                if (session_matches and (row['session'] == leader or row['pgrp'] == leader))
                or (pid in known and known[pid] == row['start_ticks'])}
    if leader in rows and (leader not in known or known[leader] == rows[leader]['start_ticks']):
        selected.add(leader)
    changed = True
    while changed:
        additions = {pid for pid, row in rows.items() if row['ppid'] in selected} - selected
        changed = bool(additions)
        selected.update(additions)
    members = [rows[pid] for pid in sorted(selected)]
    known.update({row['pid']: row['start_ticks'] for row in members})
    return {'rss_bytes': sum(row['rss_bytes'] for row in members),
            'members': members, 'vanished_during_scan': vanished,
            'unreadable_unidentified_processes': unreadable}


def files(root: Path) -> dict:
    """Count regular-file logical/allocated bytes once per inode; never follow links."""
    pending = [root]
    seen = set()
    logical = allocated = count = vanished = 0
    device = root.stat().st_dev
    while pending:
        directory = pending.pop()
        try:
            with os.scandir(directory) as entries:
                for entry in entries:
                    try:
                        info = entry.stat(follow_symlinks=False)
                    except FileNotFoundError:
                        vanished += 1
                        continue
                    if info.st_dev != device:
                        raise ValueError('artifact traverses a different filesystem')
                    if stat.S_ISLNK(info.st_mode):
                        raise ValueError('artifact symlink prevents complete resource accounting')
                    if stat.S_ISDIR(info.st_mode):
                        pending.append(Path(entry.path))
                    elif stat.S_ISREG(info.st_mode):
                        identity = (info.st_dev, info.st_ino)
                        if identity not in seen:
                            seen.add(identity)
                            logical += info.st_size
                            allocated += info.st_blocks * 512
                            count += 1
                    else:
                        raise ValueError('nonregular artifact prevents complete resource accounting')
        except FileNotFoundError:
            vanished += 1
    disk = os.statvfs(root)
    return {'logical_bytes': logical, 'allocated_bytes': allocated, 'file_count': count,
            'available_bytes': max(0, disk.f_bavail) * disk.f_frsize,
            'vanished_during_scan': vanished}


def system_telemetry(cpus: tuple[int, ...]) -> dict:
    result = {'sampled_unix_seconds': time.time(), 'load_average': list(os.getloadavg()),
              'frequency_khz': {}, 'temperatures_millicelsius': {}, 'unavailable': []}
    paths = [(f'cpu{cpu}', Path(f'/sys/devices/system/cpu/cpu{cpu}/cpufreq/scaling_cur_freq'), 'frequency_khz') for cpu in cpus]
    paths += [(str(path.relative_to('/sys/class')), path, 'temperatures_millicelsius')
              for path in [*Path('/sys/class/thermal').glob('thermal_zone*/temp'),
                           *Path('/sys/class/hwmon').glob('hwmon*/temp*_input')]]
    for label, path, group in paths:
        try:
            result[group][label] = int(path.read_text().strip())
        except (OSError, ValueError):
            result['unavailable'].append(label)
    return result


def violations(snapshot: dict, limits: Limits) -> list[str]:
    result = []
    if snapshot['processes']['rss_bytes'] > limits.rss_bytes:
        result.append('observed aggregate RSS exceeds limit')
    if max(snapshot['files']['logical_bytes'], snapshot['files']['allocated_bytes']) > limits.files_bytes:
        result.append('observed temporary/output bytes exceed limit')
    if snapshot['files']['available_bytes'] < limits.headroom_bytes:
        result.append('filesystem headroom below limit')
    if snapshot.get('invocation_elapsed_seconds', snapshot['elapsed_seconds']) > limits.seconds:
        result.append('invocation timeout')
    return result


class Guard:
    """Observe a subprocess created with start_new_session=True.

    Only one thread mutates observations. stop() joins it, then performs a final
    sample. The caller must preserve reports on every failure. This class does
    not authorize running a command or deleting its artifacts.
    """
    def __init__(self, leader: int, root: Path, limits: Limits, started: float, cpus: tuple[int, ...] = ()):
        if leader <= 1 or os.getpgid(leader) != leader or os.getsid(leader) != leader:
            raise ValueError('resource guard requires a fresh subprocess session')
        self.leader = leader
        self.root = root.resolve(strict=True)
        self.limits = limits
        self.started = started
        self.cpus = cpus
        identity = parse_process(leader, Path(f'/proc/{leader}/stat').read_text(errors='surrogateescape'))
        self.known: dict[int, int] = {leader: identity['start_ticks']}
        self.samples: list[dict] = []
        self.failure: list[str] = []
        self.observer_error: str | None = None
        self.cpu_seconds = 0.0
        self.event = threading.Event()
        self.thread = threading.Thread(target=self._run, name='resource-observer', daemon=True)
        self.watchdog = threading.Timer(max(0, limits.seconds - (time.monotonic() - started)), self._timeout)
        self.watchdog.daemon = True

    def _timeout(self):
        if 'invocation timeout' not in self.failure:
            self.failure.append('invocation timeout')
        self.kill()

    def kill(self):
        # The leader is retained by Popen until wait() reaps it; track individual
        # identities too for observed descendants that created another session.
        try:
            row = parse_process(self.leader, Path(f'/proc/{self.leader}/stat').read_text(errors='surrogateescape'))
            if row['start_ticks'] == self.known[self.leader]:
                os.killpg(self.leader, signal.SIGKILL)
        except (FileNotFoundError, ProcessLookupError):
            pass
        for pid, start in list(self.known.items()):
            try:
                row = parse_process(pid, Path(f'/proc/{pid}/stat').read_text(errors='surrogateescape'))
                if row['start_ticks'] == start:
                    os.kill(pid, signal.SIGKILL)
            except (FileNotFoundError, ProcessLookupError):
                pass

    def sample(self, finished_elapsed: float | None = None):
        begin = time.monotonic()
        snapshot = {'elapsed_seconds': begin - self.started,
                    'processes': processes(self.leader, self.known), 'files': files(self.root),
                    'system': system_telemetry(self.cpus)}
        if finished_elapsed is not None:
            snapshot['invocation_elapsed_seconds'] = finished_elapsed
        snapshot['scan_seconds'] = time.monotonic() - begin
        self.samples.append(snapshot)
        reasons = violations(snapshot, self.limits)
        for reason in reasons:
            if reason not in self.failure:
                self.failure.append(reason)
        if reasons:
            self.kill()

    def _run(self):
        cpu_start = time.thread_time()
        try:
            while not self.event.is_set():
                begin = time.monotonic()
                self.sample()
                if self.failure:
                    break
                self.event.wait(max(0, self.limits.interval_seconds - (time.monotonic() - begin)))
        except BaseException as error:
            self.observer_error = repr(error)
            self.failure.append('resource observer failed')
            self.kill()
        finally:
            self.cpu_seconds += time.thread_time() - cpu_start

    def start(self):
        self.watchdog.start()
        self.thread.start()

    def stop(self, finished_elapsed: float | None = None) -> dict:
        if finished_elapsed is None:
            finished_elapsed = time.monotonic() - self.started
        self.event.set()
        self.watchdog.cancel()
        self.watchdog.join()
        self.thread.join()
        # Joining a slow observer is outside the measured CLI duration.
        if finished_elapsed <= self.limits.seconds:
            self.failure[:] = [reason for reason in self.failure if reason != 'invocation timeout']
        try:
            self.sample(finished_elapsed)
        except BaseException as error:
            self.observer_error = repr(error)
            if 'resource observer failed' not in self.failure:
                self.failure.append('resource observer failed')
            self.kill()
        return {'limits': asdict(self.limits), 'samples': self.samples,
                'failures': self.failure, 'observer_error': self.observer_error,
                'observer_thread_cpu_seconds': self.cpu_seconds,
                'observed_peak_aggregate_rss_bytes': max((s['processes']['rss_bytes'] for s in self.samples), default=None),
                'observed_peak_logical_file_bytes': max((s['files']['logical_bytes'] for s in self.samples), default=None),
                'observed_peak_allocated_file_bytes': max((s['files']['allocated_bytes'] for s in self.samples), default=None),
                'limitations': ['sampled observations are not OS-enforced bounds',
                                'short-lived or undiscovered escaping descendants can be missed',
                                'RSS sums can count shared pages multiple times',
                                'file scans are not atomic and may miss short-lived files',
                                'unlinked open files and writes outside the workspace are not counted',
                                'PID/start-tick checks reduce but cannot eliminate kill races',
                                'GNU-time maximum RSS must also be checked; it is not simultaneous aggregate RSS']}
