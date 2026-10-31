#!/usr/bin/env python3
"""Stage already-installed pinned oracles for README benchmarks; never download."""
import argparse
from pathlib import Path
import shlex
import subprocess


def main():
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument('--cache',type=Path,required=True,help='new directory; existing paths are refused')
    p.add_argument('--umi-prefix',type=Path,required=True,help='environment with UMI-tools, samtools and OpenJDK')
    p.add_argument('--fastqc-dir',type=Path,required=True,help='unpacked FastQC 0.13.0 directory')
    p.add_argument('--seqkit',type=Path,required=True)
    p.add_argument('--fq',type=Path,required=True)
    args=p.parse_args()
    cache=args.cache.resolve(); umi=args.umi_prefix.resolve(); fastqc=args.fastqc_dir.resolve()
    if cache.exists():
        p.error('refusing to overwrite existing cache')
    for file in [umi/'bin/python3',umi/'bin/umi_tools',umi/'bin/samtools',umi/'lib/jvm/bin/java',
                 fastqc/'fastqc',args.seqkit.resolve(),args.fq.resolve()]:
        if not file.is_file():
            p.error(f'required installed tool missing: {file}')
    cache.mkdir(parents=True)
    (cache/'umi').symlink_to(umi,target_is_directory=True)
    (cache/'fastqc').symlink_to(fastqc,target_is_directory=True)
    bin_dir=cache/'bin';bin_dir.mkdir()
    def wrapper(name,command,environment=''):
        path=bin_dir/name
        path.write_text('#!/bin/sh\n'+environment+'exec '+shlex.join([str(x) for x in command])+' "$@"\n')
        path.chmod(0o755)
    lib=shlex.quote(str(cache/'umi/lib'))
    wrapper('umi_tools',[cache/'umi/bin/python3',cache/'umi/bin/umi_tools'])
    wrapper('samtools',[cache/'umi/bin/samtools'],f'export LD_LIBRARY_PATH={lib}${{LD_LIBRARY_PATH:+:$LD_LIBRARY_PATH}}\n')
    java=shlex.quote(str(cache/'umi/lib/jvm'))
    wrapper('fastqc',[cache/'fastqc/fastqc'],
            f'export JAVA_HOME={java}\nexport PATH={shlex.quote(str(cache/"umi/bin"))}:$PATH\n'
            f'export LD_LIBRARY_PATH={shlex.quote(str(cache/"umi/lib/jvm/lib"))}:{lib}${{LD_LIBRARY_PATH:+:$LD_LIBRARY_PATH}}\n')
    (bin_dir/'seqkit').symlink_to(args.seqkit.resolve())
    (bin_dir/'fq').symlink_to(args.fq.resolve())
    expected={'umi_tools':'UMI-tools version: 1.1.6','samtools':'samtools 1.24',
              'fastqc':'FastQC v0.13.0','seqkit':'seqkit v2.14.0','fq':'fq v0.12.0 '}
    for name,version in expected.items():
        result=subprocess.run([str(bin_dir/name),'version' if name=='seqkit' else '--version'],
                              capture_output=True,text=True,check=True,timeout=30)
        banner=(result.stdout or result.stderr).strip().splitlines()[0]
        if not (banner.startswith(version) if name=='fq' else banner==version):
            raise SystemExit(f'wrong reference: {name}: {banner}; cache is not marked ready')
        print(banner)
    (cache/'.ready').write_text('Pinned version preflight passed; binaries are installed externally.\n')
    print(f'Ready: --oracle-dir {bin_dir} --toolchain-cache {cache}')


if __name__=='__main__':
    main()
