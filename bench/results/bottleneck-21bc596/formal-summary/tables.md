| Workload | Repeated population | Input records | Input GiB | CPU budget | Upstream s, median [min–max] | Current s, median [min–max] | Paired upstream/current |
|---|---|---:|---:|---:|---:|---:|---:|
| SeqKit stats -a -T | real FASTQ subset | 120M | 41.10 | 1 | 60.36 [58.83–60.47] | 21.44 [21.42–21.44] | 2.82× |
| SeqKit stats -a -T | real FASTQ subset | 120M | 41.10 | 2 | 61.40 [61.06–64.68] | 21.47 [21.42–21.60] | 2.86× |
| SeqKit stats -a -T | real FASTQ subset | 120M | 41.10 | 4 | 61.86 [61.63–61.87] | 21.79 [21.70–21.79] | 2.84× |
| SeqKit stats -a -T | synthetic FASTQ | 120M | 36.21 | 1 | 45.52 [44.70–46.08] | 19.59 [19.58–19.60] | 2.32× |
| SeqKit stats -a -T | synthetic FASTQ | 120M | 36.21 | 2 | 46.32 [46.17–47.93] | 19.59 [19.58–19.73] | 2.36× |
| SeqKit stats -a -T | synthetic FASTQ | 120M | 36.21 | 4 | 46.50 [46.43–46.73] | 19.91 [19.90–19.91] | 2.34× |
| samtools sort | synthetic BAM | 86M | 0.39 | 1 | 49.14 [47.62–50.82] | 96.65 [92.94–100.80] | 0.51× |
| samtools sort | synthetic BAM | 86M | 0.39 | 2 | 53.51 [53.48–53.86] | 82.42 [81.59–82.98] | 0.65× |
| samtools sort | synthetic BAM | 86M | 0.39 | 4 | 23.35 [23.32–23.52] | 82.20 [82.00–82.40] | 0.28× |
