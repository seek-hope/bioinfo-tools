#!/usr/bin/env python3
"""Timed child: launch a frozen number of fresh CLIs; retain every artifact.

Prepared directories and plan validation belong to the parent. No input
conditioning, output comparison, hashing, fsync or output deletion happens here.
Python startup, launch/redirection overhead and event writes are inside timing.
"""
import json
import os
from pathlib import Path
import subprocess
import sys


def main():
    plan_path = Path(sys.argv[1])
    plan = json.loads(plan_path.read_text())
    root = plan_path.parent
    with (root/'events.jsonl').open('x') as events:
        for index in range(plan['count']):
            directory = root/f'run-{index:06d}'
            command = [str(directory/part['run_path']) if isinstance(part, dict) else part
                       for part in plan['argv']]
            env = dict(os.environ, TMPDIR=str(directory/'tmp'))
            with (directory/'stdout').open('xb') as stdout, (directory/'stderr').open('xb') as stderr:
                with open(plan['stdin'] or os.devnull, 'rb') as stdin:
                    process = subprocess.Popen(command, cwd=directory, env=env,
                                               stdin=stdin, stdout=stdout, stderr=stderr)
                    affinity = sorted(os.sched_getaffinity(process.pid))
                    stat = Path(f'/proc/{process.pid}/stat').read_text(errors='surrogateescape')
                    start_ticks = int(stat[stat.rfind(')')+1:].split()[19])
                    code = process.wait()
            events.write(json.dumps({'index': index, 'pid': process.pid, 'start_ticks': start_ticks, 'command': command,
                                     'returncode': code, 'initial_affinity': affinity})+'\n')
            events.flush()
            if code != plan['expected_exit']:
                return 1
    return 0


if __name__ == '__main__':
    raise SystemExit(main())
