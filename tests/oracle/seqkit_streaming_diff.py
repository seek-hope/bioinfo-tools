#!/usr/bin/env python3
"""Direct SeqKit 2.14.0 range/tab2fx comparisons; never writes repository fixtures.

Compare exit status, exact stdout/output payload and diagnostic bodies. Only
ANSI escapes and the tool-specific ERROR:/[ERRO] prefix are normalized. Compressed
success output is decoded; backend bytes and incomplete compressed prefixes are
not asserted identical. All inputs are generated in a temporary directory.
"""

import argparse
import bz2
from collections import Counter
import gzip
import lzma
from pathlib import Path
import re
import subprocess
import tempfile


def diagnostic(data):
    data = re.sub(rb"\x1b\[[0-9;]*m", b"", data)
    return re.sub(rb"^(?:ERROR:|\[ERRO\]) ", b"", data, flags=re.M)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--seqkit", default="tests/ci/env/bin/seqkit")
    parser.add_argument("--rust", default="target/debug/rust-fx")
    parser.add_argument("--command", choices=("range", "tab2fx"))
    args = parser.parse_args()
    reference, candidate = [str(Path(p).resolve()) for p in (args.seqkit, args.rust)]
    version = subprocess.check_output([reference, "version"]).decode().strip()
    if version != "seqkit v2.14.0":
        raise SystemExit(f"wrong reference: {version}")
    counts = Counter()
    failures = Counter()
    with tempfile.TemporaryDirectory(prefix="bioinfo-seqkit-streaming-") as directory:
        root = Path(directory)

        def case(command, label, options=(), data=None, files=None, output=None, decode=None):
            if args.command and command != args.command:
                return
            inputs = []
            if files is not None:
                for number, payload in enumerate(files):
                    path = root / f"input-{number}"
                    path.write_bytes(payload)
                    inputs.append(str(path))
            results = []
            for binary in (reference, candidate):
                target = root / output if output else None
                if target is not None:
                    target.unlink(missing_ok=True)
                argv = [binary, command, *options, *inputs]
                if target is not None:
                    argv += ["-o", str(target)]
                run = subprocess.run(argv, input=data, capture_output=True, timeout=20)
                artifact = target.read_bytes() if target is not None and target.exists() else None
                if artifact is not None and decode is not None and run.returncode == 0:
                    artifact = decode(artifact)
                results.append((run.returncode, run.stdout, diagnostic(run.stderr), artifact))
            if results[0] != results[1]:
                for which, result in zip(("upstream", "rust"), results):
                    code, stdout, stderr, artifact = result
                    print(label, which, code, f"stdout={len(stdout)}", repr(stdout[:120]),
                          repr(stderr), f"artifact={None if artifact is None else len(artifact)}")
                failures[command] += 1
            counts[command] += 1

        ranges = ("1:1", "1:10", "2:3", "5:3", "2:-1", "20:-1", "-1:-1",
                  "-2:-1", "-5:-3", "-9:-7", "-5:-5", "-1000000000:-1", "+1:2", "1:2:3")
        for size in (0, 1, 4):
            fasta = b"".join(f">r{i} description\nACGT\n".encode() for i in range(size))
            fastq = b"".join(f"@r{i} description\nAC\nGT\n+discard\nII\nJJ\n".encode()
                             for i in range(size))
            for layout, payload in (("fasta", fasta), ("wrapped-fastq", fastq)):
                for region in ranges:
                    for mode in ("stdin", "file"):
                        case("range", f"{layout}/{size}/{region}/{mode}", ("-r", region),
                             **({"data": payload} if mode == "stdin" else {"files": [payload]}))
        for region in ("", "0:1", "1:0", "-2:3", "2:-2", "-1:-2", ":", "1:",
                       "x1:2", "1:+2", "9223372036854775808:1", "-9223372036854775808:-1"):
            case("range", f"invalid/{region}", ("-r", region), data=b">a\nAC\n", output="range.fa")
        case("range", "missing-range", data=b">a\nAC\n", output="range.fa")
        for region in ("2:3", "2:-1", "-2:-1", "-9:-6"):
            case("range", f"multi/{region}", ("-r", region),
                 files=[b">a\nA\n>b\nCC\n", b">c\nG\n>d\nTT\n>e\nNN\n"])
        for first, second in ((b"@q\nACGT\n+x\nIIII\n", b">f\nAAAA\n"),
                              (b">f\nAAAA\n", b"@q\nACGT\n+x\nIIII\n")):
            for width in ("0", "2", "60"):
                case("range", "mixed-formats/" + width, ("-r", "1:-1", "-w", width),
                     files=[first, second])
        for width in ("0", "2", "60"):
            case("range", "marker-header/" + width, ("-r", "1:-1", "-w", width),
                 data=b">>id\nACGT\n>@id\nACGT\n")
        for prefix in (0, 10, 14000):
            case("range", f"bad-quality/{prefix}", ("-r", "1:-1"),
                 data=b"@a\nAC\n+x\nII\n" * prefix + b"@b\nAC\n+\nI\n", output="range.fq")

        tables = [b"a\tAC\n", b"a\tAC\tII\n", b"a\tAC\tI\n", b"a\tA\tII\n",
                  b"a\tAC\tII\textra\n", b"a\tAC\t\n", b"a\t\n", b"a\t\t\n",
                  b"\tAC\n", b"\t\n", b"\n# skip\n// skip\na\tAC\n", b"bad\n",
                  b"a\t\xffG\n", b"a\tAC\t\x00\xff\n", b"a\tAC\r\r\n", b"a\tAC",
                  b"a\tAC\tb\nb\tGT\t\nc\tAC\nd\t\t\n", b" \n", b""]
        for number, payload in enumerate(tables):
            for mode in ("stdin", "file"):
                case("tab2fx", f"row/{number}/{mode}",
                     **({"data": payload} if mode == "stdin" else {"files": [payload]}))
        for width in ("0", "1", "3", "60"):
            case("tab2fx", "wrap/" + width, ("-w", width), data=b"a\tACGT\nb\t\n")
        for prefix in ("", ";,//", ",;", '"#","//"', '"a,b"', '"a""b"',
                       "#\n", "#\r\n", '"#"\n', "\n#", '"#"\nignored',
                       '"#\r\nfoo"', '"#"\r'):
            case("tab2fx", "prefix/" + prefix, ("-p", prefix),
                 data=b"# skip\tA\n// skip\tA\n; skip\tA\na,b header\tA\na\"b\tA\n")
        case("tab2fx", "repeated-prefix", ("-p", ";", "-p", "//,#"),
             data=b"; skip\n# skip\n// skip\na\tA\n")
        case("tab2fx", "file-state", files=[b"a\tAC\tII\nb\tAC\t\n", b"c\tAC\t\n"])
        for value in ("1K", "1k", "1.5K", "1024B", "1e3", " 1 K ", "0", "-1", "K", "1KB", ""):
            case("tab2fx", "buffer-value/" + value, ("-b", value), data=b"a\tAC\n", output="table.fa")
        for newline in (b"", b"\n", b"\r\n"):
            for length in (1020, 1021, 1022):
                case("tab2fx", f"limit/{length}/{newline!r}", ("-b", "1K", "-w", "0"),
                     data=b"a\t" + b"A" * length + newline, output="table.fa")
        for length in (1, 65536, 65537, 140000):
            case("tab2fx", f"large-record/{length}", ("-w", "0"),
                 data=b"a\t" + b"A" * length + b"\nbad\n", output="table.fa")
        for prefix in (10, 10922, 10923, 30000):
            for failure in (b"bad\n", b"b\t" + b"A" * 2000 + b"\n"):
                case("tab2fx", f"late-error/{prefix}/{len(failure)}", ("-b", "1K"),
                     data=b"a\tAC\n" * prefix + failure, output="table.fa")
        case("tab2fx", "cross-file-error", files=[b"a\tAC\n" * 12000, b"bad\n"], output="table.fa")

        for command, options, payload in (("range", ("-r", "2:-1"), b">a\nAC\n>b\nGT\n"),
                                           ("tab2fx", (), b"a\tAC\nb\tGT\n")):
            for extension, encode, decode in (("gz", gzip.compress, gzip.decompress),
                                               ("bz2", bz2.compress, bz2.decompress),
                                               ("xz", lzma.compress, lzma.decompress)):
                case(command, "compressed-input/" + extension, options, files=[encode(payload)])
                case(command, "compressed-stdin/" + extension, options, data=encode(payload))
                case(command, "compressed-output/" + extension, options, data=payload,
                     output="output.fa." + extension, decode=decode)

    for command, count in sorted(counts.items()):
        failed = failures[command]
        status = "FAIL" if failed else "PASS"
        print(f"{status} SeqKit 2.14.0 {command}: {count-failed}/{count} differentials")
    if failures:
        raise SystemExit(1)


if __name__ == "__main__":
    main()
