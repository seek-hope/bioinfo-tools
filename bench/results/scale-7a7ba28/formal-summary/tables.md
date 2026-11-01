| Workload | Repeated population | Input records | Input GiB | CPU budget | Upstream s, median [min–max] | Current s, median [min–max] | Paired upstream/current |
|---|---|---:|---:|---:|---:|---:|---:|
| cutadapt plain | real FASTQ subset | 54M | 18.50 | 1 | 127.21 [120.21–141.27] | 145.22 [142.22–157.21] | 0.89× |
| cutadapt plain | real FASTQ subset | 54M | 18.50 | 2 | 99.02 [95.21–108.38] | 147.06 [143.26–175.30] | 0.66× |
| cutadapt plain | real FASTQ subset | 54M | 18.50 | 4 | 106.41 [78.99–106.69] | 138.99 [113.83–156.92] | 0.69× |
| cutadapt plain | synthetic FASTQ | 46M | 13.88 | 1 | 125.39 [113.53–132.81] | 112.06 [94.95–119.51] | 1.05× |
| cutadapt plain | synthetic FASTQ | 46M | 13.88 | 2 | 84.83 [66.85–92.55] | 121.61 [105.32–149.98] | 0.63× |
| cutadapt plain | synthetic FASTQ | 46M | 13.88 | 4 | 65.63 [61.38–72.03] | 72.77 [65.97–130.40] | 0.84× |
| cutadapt gzip6 output | real FASTQ subset | 8M | 2.74 | 1 | 65.34 [65.16–65.40] | 58.41 [58.32–58.62] | 1.12× |
| cutadapt gzip6 output | real FASTQ subset | 8M | 2.74 | 2 | 53.84 [53.60–53.85] | 29.47 [29.38–29.53] | 1.83× |
| cutadapt gzip6 output | real FASTQ subset | 8M | 2.74 | 4 | 21.63 [21.52–21.75] | 16.83 [16.76–16.87] | 1.28× |
| SeqKit stats -a -T | real FASTQ subset | 30M | 10.28 | 1 | 15.61 [15.51–15.69] | 15.47 [15.47–15.54] | 1.00× |
| SeqKit stats -a -T | real FASTQ subset | 30M | 10.28 | 2 | 15.36 [15.21–15.37] | 15.68 [15.55–15.68] | 0.98× |
| SeqKit stats -a -T | real FASTQ subset | 30M | 10.28 | 4 | 15.37 [15.36–15.44] | 15.92 [15.92–15.99] | 0.96× |
| SeqKit stats -a -T | synthetic FASTQ | 41M | 12.37 | 1 | 16.13 [15.91–16.15] | 21.21 [21.18–21.22] | 0.76× |
| SeqKit stats -a -T | synthetic FASTQ | 41M | 12.37 | 2 | 15.99 [15.72–16.11] | 21.47 [21.37–21.58] | 0.74× |
| SeqKit stats -a -T | synthetic FASTQ | 41M | 12.37 | 4 | 15.85 [15.82–16.15] | 21.93 [21.86–22.44] | 0.72× |
| SeqKit fq2fa | synthetic FASTQ | 146M | 44.06 | 1 | 155.21 [138.19–179.92] | 134.25 [119.64–138.81] | 1.16× |
| SeqKit fq2fa | synthetic FASTQ | 146M | 44.06 | 4 | 106.11 [103.83–132.82] | 135.81 [110.02–139.59] | 0.95× |
| fq lint | synthetic FASTQ | 102M | 30.78 | 1 | 28.35 [26.81–29.14] | 23.06 [22.60–23.28] | 1.23× |
| fq lint | synthetic FASTQ | 102M | 30.78 | 4 | 29.53 [29.41–30.40] | 27.34 [25.07–27.74] | 1.11× |
| samtools flagstat | synthetic BAM | 755M | 3.47 | 1 | 34.51 [28.53–34.66] | 69.57 [65.09–76.03] | 0.45× |
| samtools flagstat | synthetic BAM | 755M | 3.47 | 4 | 16.38 [16.35–16.43] | 60.60 [60.33–61.01] | 0.27× |
| samtools sort | synthetic BAM | 86M | 0.39 | 1 | 43.66 [42.45–44.77] | 94.89 [93.55–97.71] | 0.46× |
| samtools sort | synthetic BAM | 86M | 0.39 | 2 | 52.44 [52.40–52.47] | 91.77 [91.50–91.93] | 0.57× |
| samtools sort | synthetic BAM | 86M | 0.39 | 4 | 23.18 [23.14–23.18] | 91.61 [91.11–91.68] | 0.25× |
| UMI-tools directional dedup | synthetic BAM | 71M | 0.16 | 1 | 228.66 [223.20–237.04] | 15.27 [15.26–15.29] | 14.98× |
| UMI-tools directional dedup | synthetic BAM | 71M | 0.16 | 4 | 215.23 [215.09–216.18] | 14.80 [14.76–14.83] | 14.54× |
| FastQC (descriptive) | real FASTQ subset | 9M | 3.08 | 1 | 34.31 [34.29–34.31] | 40.15 [40.13–40.19] | — unequal report work |
| FastQC (descriptive) | real FASTQ subset | 9M | 3.08 | 4 | 15.09 [15.09–15.11] | 41.24 [41.02–41.45] | — unequal report work |
