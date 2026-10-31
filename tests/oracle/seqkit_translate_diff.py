#!/usr/bin/env python3
"""Compare translate directly with SeqKit 2.14.0, using temporary generated inputs.

Compare exact exit status, stdout, file payload and diagnostic bodies. Only ANSI
colour and ERROR:/[ERRO] prefixes are normalized. Successful compressed output
is decoded; failed compressed streams are checked for failure, not codec bytes.
No fixtures, existing oracle expectations or reference output files are edited.
"""

import argparse
import bz2
import gzip
import hashlib
import itertools
import json
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
    parser.add_argument("--report", type=Path)
    args = parser.parse_args()
    reference, candidate = [str(Path(p).resolve()) for p in (args.seqkit, args.rust)]
    version = subprocess.check_output([reference, "version"]).decode().strip()
    if version != "seqkit v2.14.0":
        raise SystemExit(f"wrong reference: {version}")
    results = []
    with tempfile.TemporaryDirectory(prefix="bioinfo-translate-") as directory:
        root = Path(directory)

        def case(label, options=(), data=None, files=None, output=None, decode=None):
            inputs = []
            if files is not None:
                for number, payload in enumerate(files):
                    path = root / f"input-{number}"
                    path.write_bytes(payload)
                    inputs.append(str(path))
            observed = []
            for binary in (reference, candidate):
                target = root / output if output else None
                if target is not None:
                    target.unlink(missing_ok=True)
                argv = [binary, "translate", *options, *inputs]
                if target is not None:
                    argv += ["-o", str(target)]
                run = subprocess.run(argv, input=data, capture_output=True, timeout=30)
                artifact = target.read_bytes() if target is not None and target.exists() else None
                if artifact is not None and decode is not None:
                    artifact = decode(artifact) if run.returncode == 0 else None
                observed.append((run.returncode, run.stdout, diagnostic(run.stderr), artifact))
            passed = observed[0] == observed[1]
            # Observation hashes add audit detail only. The exact comparison,
            # printed failures and nonzero exit on any difference stay intact.
            observations = []
            for code, stdout, stderr, artifact in observed:
                observations.append({
                    "code": code,
                    "stdout_sha256": hashlib.sha256(stdout).hexdigest(),
                    "stderr_sha256": hashlib.sha256(stderr).hexdigest(),
                    "artifact_sha256": None if artifact is None else hashlib.sha256(artifact).hexdigest(),
                })
            results.append({"case": label, "passed": passed, "observations": observations})
            if not passed:
                for which, (code, stdout, stderr, artifact) in zip(("upstream", "rust"), observed):
                    print(label, which, code, f"stdout={len(stdout)}", repr(stdout[:160]),
                          repr(stderr), f"artifact={None if artifact is None else len(artifact)}")

        tables = (1, 2, 3, 4, 5, 6, 9, 10, 11, 12, 13, 14, 16,
                  21, 22, 23, 24, 25, 26, 27, 28, 29, 30, 31)
        codons = [bytes(codon) for codon in itertools.product(b"ACGTRYSWKMBDHVN", repeat=3)]
        # Every ambiguity codon in every supported table, also in reverse frames.
        payload = b"".join(b">" + codon + b" description\n" + codon + b"\n" for codon in codons)
        for table in tables:
            for mode in ((), ("-M",), ("-f", "6", "-F"), ("--trim", "--clean")):
                case(f"codons/{table}/{mode}", ("-T", str(table), *mode), data=payload)
            for mode in ("-l", "-L"):
                case(f"listing/{table}/{mode}", (mode, str(table)), data=b"")
        for listing in (("-l", "0"), ("-L", "0"), ("-l", "7"), ("-L", "7"),
                        ("-l", "1", "-L", "2"), ("-l", "0", "-L", "2")):
            case(f"listing/{listing}", listing, data=b"")

        sequences = (b"", b"A", b"AC", b"ATG", b"A?G", b"AXG", b"A.G", b"A*G", b"A G",
                     b"---", b"A-G", b"ATGTAACTG", b"ATGNNNCCC", b"GTGTAATTG",
                     b"ATGRYSWKMBDHVN", b"AUGUUG", b"ATGUUU", b"ATG\xffCG")
        modes = ((), ("-e",), ("-x",), ("--trim",), ("--clean", "--trim"),
                 ("-M",), ("-f", "6", "-e", "-F"), ("-f", "6", "-s", "-e"))
        for number, sequence in enumerate(sequences):
            for prefix in (b"", b">good\nATG\n"):
                for mode in modes:
                    case(f"sequence/{number}/{bool(prefix)}/{mode}", mode,
                         data=prefix + b">read description\n" + sequence + b"\n")

        for frame in ("1", "2", "3", "-1", "-2", "-3", "6", "3,1,-2,1", ""):
            for length in (3, 10, 27, 61):
                data = b">orf desc\n" + (b"ATGTAATTGCCCNNN" * 6)[:length] + b"\n"
                for mode in (("-F",), ("-s",), ("-s", "-M", "-m", "2"), ("-s", "--clean")):
                    case(f"frames/{frame}/{length}/{mode}", ("-f", frame, *mode), data=data)
        for options in (("-f", "0"), ("-f", "-4"), ("-f", "x"), ("-f", "1,,2"),
                        ("-f", "6,x"), ("-f", "1", "-f", "-1,2"), ("-T", "0"),
                        ("-T", "-1"), ("-T", "7"), ("-m", "-1")):
            case(f"arguments/{options}", options, data=b">a\nATG\n", output="arguments.fa")
        for name in (b"r desc", b"r\tdesc", b"r  desc", b"r\t \tdesc", b">r", b"@r", b"r \xff"):
            for mode in ((), ("-F",), ("-s",), ("-e",)):
                case(f"header/{name!r}/{mode}", mode, data=b">" + name + b"\nATG\n")
        for width in ("0", "1", "3", "60"):
            for prefix in (10, 10922, 10923, 30000):
                case(f"late-error/{width}/{prefix}", ("-w", width),
                     data=b">a\nATG\n" * prefix + b">bad\nA?G\n", output="late.fa")
            for length in (65535, 65536, 65537, 140000):
                case(f"large-record-error/{width}/{length}", ("-w", width),
                     data=b">a\n" + b"ATG" * length + b"\n>bad\nA?G\n", output="large.fa")
        for length in (9999, 10000, 10001):
            case(f"alphabet-prefix/{length}", ("-x",), data=b">a\n" + b"A" * length + b"?GG\n")

        for mode in modes:
            for second in (b"", b">protein\nELVIS\n", b">unknown\nA?G\n", b">empty\n",
                           b">rna\nAUGUUG\n", b"@q desc\nATG\nCCC\n+discard\nIII\nJJJ\n"):
                case(f"multi-file/{second!r}/{mode}", mode, files=[b">dna\nATG\n", second])
        for length in (3, 196608):
            for extra in (b"I\n", b"\nI\n", b"\n\n", b"@next\nATG\n+\nIII\n"):
                payload = b"@a\n" + b"ATG" * (length // 3) + b"\n+\n" + b"I" * length + b"\n" + extra
                for mode in ("stdin", "file"):
                    case(f"quality-boundary/{length}/{extra!r}/{mode}", ("-w", "0"),
                         **({"data": payload} if mode == "stdin" else {"files": [payload]}))
        for data in (b"", b"\n", b"plain text\n", b"@q\nATG\n+\nII\n",
                     b"@q desc\nAT\nGCCC\n+\nII\nIIII\n"):
            for mode in ("stdin", "file"):
                case(f"input/{data!r}/{mode}", **({"data": data} if mode == "stdin" else {"files": [data]}))
        for extension, encode, decode in (("gz", gzip.compress, gzip.decompress),
                                           ("bz2", bz2.compress, bz2.decompress),
                                           ("xz", lzma.compress, lzma.decompress)):
            for payload in (b">a\nATG\n", b">a\nATG\n>bad\nA?G\n", b""):
                case(f"compression/{extension}/{payload!r}/file", files=[encode(payload)])
                case(f"compression/{extension}/{payload!r}/stdin", data=encode(payload))
                case(f"compression/{extension}/{payload!r}/output", data=payload,
                     output="translated.fa." + extension, decode=decode)

    failed = sum(not result["passed"] for result in results)
    print(f"translate: {len(results) - failed}/{len(results)} direct cases passed")
    if args.report:
        args.report.write_text(json.dumps({"reference": version, "candidate": candidate,
                                          "passed": len(results) - failed, "total": len(results),
                                          "cases": results}, indent=2) + "\n")
    return bool(failed)


if __name__ == "__main__":
    raise SystemExit(main())
