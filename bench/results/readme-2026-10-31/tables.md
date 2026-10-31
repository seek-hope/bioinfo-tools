# Complete README benchmark tables

Completed conditions use medians of five fresh-process executions after one warm-up; failed/incomplete conditions remain visible. Parentheses show observed min–max, not confidence intervals. Ratios are medians of paired upstream/native times; values below 1 mean native is slower. RSS is the maximum GNU time RSS across the five executions, not aggregate process-tree memory.

## Same-output or validation-verdict workloads

| Dataset | Command | Native s (min–max) | Upstream s (min–max) | Upstream/native ratio | Peak RSS MiB native/upstream |
| --- | --- | ---: | ---: | ---: | ---: |
| synthetic-100000 | cutadapt: plain output (1 CPU) | 0.273 (0.271–0.278) | 0.268 (0.266–0.272) | 0.99× | 8.1 / 18.7 |
| synthetic-100000 | SeqKit stats -a -T | 0.220 (0.217–0.222) | 0.051 (0.049–0.071) | 0.23× | 5.6 / 36.1 |
| synthetic-100000 | SeqKit fq2fa | 0.021 (0.021–0.022) | 0.023 (0.022–0.024) | 1.10× | 5.5 / 34.9 |
| synthetic-100000 | fq lint | 0.043 (0.042–0.044) | 0.021 (0.021–0.021) | 0.49× | 11.6 / 4.4 |
| synthetic-1000000 | cutadapt: plain output (1 CPU) | 2.767 (2.757–2.770) | 2.271 (2.239–2.348) | 0.82× | 8.8 / 18.7 |
| synthetic-1000000 | SeqKit stats -a -T | 2.208 (2.204–2.212) | 0.425 (0.422–0.426) | 0.19× | 5.6 / 35.4 |
| synthetic-1000000 | SeqKit fq2fa | 0.200 (0.196–0.205) | 0.139 (0.136–0.144) | 0.70× | 5.5 / 36.5 |
| synthetic-1000000 | fq lint | 0.577 (0.566–0.615) | 0.194 (0.194–0.197) | 0.34× | 108.1 / 4.4 |
| synthetic-1000000 | cutadapt: gzip level 6 (1 CPU) | 6.411 (6.407–6.458) | 5.957 (5.930–6.038) | 0.93× | 11.1 / 18.9 |
| synthetic-1000000 | cutadapt: plain output (4 CPUs) | 0.757 (0.747–0.761) | 0.903 (0.881–0.925) | 1.19× | 11.4 / 38.9 |
| real-100000 | cutadapt: plain output (1 CPU) | 0.289 (0.285–0.291) | 0.227 (0.223–0.245) | 0.78× | 8.8 / 18.6 |
| real-100000 | SeqKit stats -a -T | 0.220 (0.219–0.221) | 0.062 (0.062–0.065) | 0.28× | 5.7 / 36.6 |
| real-100000 | SeqKit fq2fa | 0.022 (0.022–0.023) | 0.035 (0.034–0.037) | 1.56× | 5.4 / 35.3 |
| real-100000 | fq lint | 0.045 (0.045–0.046) | 0.022 (0.021–0.022) | 0.48× | 16.0 / 4.4 |
| real-1000000 | cutadapt: plain output (1 CPU) | 2.872 (2.861–2.888) | 1.840 (1.792–1.844) | 0.64× | 9.7 / 18.7 |
| real-1000000 | SeqKit stats -a -T | 2.225 (2.214–2.234) | 0.566 (0.559–0.566) | 0.25× | 5.6 / 36.6 |
| real-1000000 | SeqKit fq2fa | 0.212 (0.209–0.213) | 0.270 (0.267–0.279) | 1.29× | 5.4 / 34.3 |
| real-1000000 | fq lint | 0.581 (0.574–0.582) | 0.201 (0.197–0.201) | 0.34× | 150.2 / 4.4 |
| real-1000000 | cutadapt: gzip level 6 (1 CPU) | 9.438 (9.352–9.752) | 8.573 (8.514–8.832) | 0.91× | 13.0 / 19.0 |
| real-1000000 | cutadapt: plain output (4 CPUs) | 0.779 (0.776–0.789) | 0.772 (0.768–0.831) | 0.99× | 13.2 / 36.3 |
| synthetic-bam-100000 | samtools flagstat | 0.010 (0.010–0.010) | 0.008 (0.008–0.008) | 0.79× | 4.1 / 4.1 |
| synthetic-bam-100000 | samtools sort (32 MiB chunks) | 0.171 (0.168–0.173) | 0.042 (0.042–0.043) | 0.25× | 43.4 / 30.5 |
| synthetic-bam-100000 | UMI-tools dedup | 0.045 (0.045–0.046) | 1.275 (1.263–1.295) | 28.24× | 37.9 / 181.3 |
| synthetic-bam-1000000 | samtools flagstat | 0.093 (0.090–0.094) | 0.038 (0.038–0.038) | 0.41× | 4.2 / 4.2 |
| synthetic-bam-1000000 | samtools sort (32 MiB chunks) | 1.701 (1.677–1.710) | 0.614 (0.603–0.618) | 0.36× | 43.5 / 54.8 |
| synthetic-bam-1000000 | UMI-tools dedup | 0.453 (0.451–0.464) | 5.116 (5.039–5.154) | 11.20× | 256.3 / 326.8 |
| synthetic-10000000 | SeqKit fq2fa | 1.980 (1.948–1.986) | 1.289 (1.276–1.344) | 0.65× | 5.4 / 34.7 |
| synthetic-10000000 | fq lint | 6.467 (6.438–6.505) | 1.959 (1.947–1.963) | 0.30× | 829.4 / 4.4 |
| synthetic-bam-10000000 | samtools flagstat | 0.960 (0.959–0.973) | 0.374 (0.367–0.377) | 0.39× | 4.1 / 4.1 |

## FastQC: descriptive timings, unequal report work

| Dataset | Command | Native s (min–max) | Upstream s (min–max) | Upstream/native ratio | Peak RSS MiB native/upstream |
| --- | --- | ---: | ---: | ---: | ---: |
| synthetic-100000 | FastQC | 0.890 (0.881–0.897) | 3.179 (2.177–3.194) | not comparable | 15.0 / 254.2 |
| synthetic-1000000 | FastQC | 9.290 (9.193–9.318) | 6.188 (6.181–6.203) | not comparable | 110.0 / 298.4 |
| real-100000 | FastQC | 0.902 (0.894–0.913) | 3.185 (3.184–3.199) | not comparable | 15.4 / 309.5 |
| real-1000000 | FastQC | 9.306 (9.239–9.375) | 6.197 (6.178–7.190) | not comparable | 92.1 / 322.9 |

Independent checks here cover total reads, displayed total bases and length. Complete raw module differences are retained in JSON. These rows do not establish full QC equivalence.

| Dataset | Modules raw-equal in every measured pair | Differing modules / missing comparison |
| --- | ---: | --- |
| synthetic-100000 | 9/10 | Per sequence GC content |
| synthetic-1000000 | 8/10 | Per sequence GC content, Sequence Duplication Levels |
| real-100000 | 10/11 | Per sequence GC content |
| real-1000000 | 9/11 | Per sequence GC content, Sequence Duplication Levels |

## Throughput, CPU, temporary storage and payload size

Each pair of values is native / upstream. Throughput is input records divided by median elapsed time. CPU is median user+system seconds. Temporary storage is the maximum sampled value; short-lived files can be missed. Payload is the measured output file size range, so QC means the data table, not its whole report bundle; raw JSON also records bundle output bytes.

| Dataset | Command | Million records/s | CPU s | Sampled temp MiB | Payload bytes (min–max) |
| --- | --- | ---: | ---: | ---: | ---: |
| synthetic-100000 | cutadapt: plain output (1 CPU) | 0.366 / 0.373 | 0.260 / 0.260 | 0.000 / 0.000 | 30885218–30885218 / 30885218–30885218 |
| synthetic-100000 | SeqKit stats -a -T | 0.454 / 1.972 | 0.210 / 0.040 | 0.000 / 0.000 | 259–259 / 259–259 |
| synthetic-100000 | SeqKit fq2fa | 4.743 / 4.328 | 0.010 / 0.010 | 0.000 / 0.000 | 17100000–17100000 / 17100000–17100000 |
| synthetic-100000 | fq lint | 2.331 / 4.764 | 0.030 / 0.010 | 0.000 / 0.000 | 783–783 / 1047–1047 |
| synthetic-100000 | FastQC | 0.112 / 0.031 | 0.870 / 1.760 | 0.000 / 0.000 | 8707–8707 / 8501–8501 |
| synthetic-1000000 | cutadapt: plain output (1 CPU) | 0.361 / 0.440 | 2.750 / 2.250 | 0.000 / 0.000 | 308853838–308853838 / 308853838–308853838 |
| synthetic-1000000 | SeqKit stats -a -T | 0.453 / 2.355 | 2.190 / 0.410 | 0.000 / 0.000 | 263–263 / 263–263 |
| synthetic-1000000 | SeqKit fq2fa | 4.996 / 7.185 | 0.190 / 0.130 | 0.000 / 0.000 | 171000000–171000000 / 171000000–171000000 |
| synthetic-1000000 | fq lint | 1.733 / 5.153 | 0.570 / 0.180 | 0.000 / 0.000 | 784–784 / 1048–1048 |
| synthetic-1000000 | FastQC | 0.108 / 0.162 | 9.260 / 5.580 | 0.000 / 0.000 | 9730–9730 / 9559–9559 |
| synthetic-1000000 | cutadapt: gzip level 6 (1 CPU) | 0.156 / 0.168 | 6.380 / 5.930 | 0.000 / 0.000 | 49719103–49719103 / 48902164–48902164 |
| synthetic-1000000 | cutadapt: plain output (4 CPUs) | 1.320 / 1.107 | 2.980 / 3.230 | 0.000 / 0.000 | 308853838–308853838 / 308853838–308853838 |
| real-100000 | cutadapt: plain output (1 CPU) | 0.346 / 0.441 | 0.280 / 0.210 | 0.000 / 0.000 | 36762087–36762087 / 36762087–36762087 |
| real-100000 | SeqKit stats -a -T | 0.456 / 1.602 | 0.210 / 0.050 | 0.000 / 0.000 | 255–255 / 255–255 |
| real-100000 | SeqKit fq2fa | 4.491 / 2.839 | 0.010 / 0.020 | 0.000 / 0.000 | 21477761–21477761 / 21477761–21477761 |
| real-100000 | fq lint | 2.212 / 4.595 | 0.030 / 0.010 | 0.000 / 0.000 | 783–783 / 1047–1047 |
| real-100000 | FastQC | 0.111 / 0.031 | 0.890 / 2.010 | 0.000 / 0.000 | 84353–84353 / 83967–83967 |
| real-1000000 | cutadapt: plain output (1 CPU) | 0.348 / 0.543 | 2.850 / 1.820 | 0.000 / 0.000 | 367627175–367627175 / 367627175–367627175 |
| real-1000000 | SeqKit stats -a -T | 0.450 / 1.768 | 2.200 / 0.560 | 0.000 / 0.000 | 260–260 / 260–260 |
| real-1000000 | SeqKit fq2fa | 4.722 / 3.710 | 0.200 / 0.260 | 0.000 / 0.000 | 214775653–214775653 / 214775653–214775653 |
| real-1000000 | fq lint | 1.723 / 4.986 | 0.560 / 0.190 | 0.000 / 0.000 | 784–784 / 1048–1048 |
| real-1000000 | FastQC | 0.107 / 0.161 | 9.260 / 5.750 | 0.000 / 0.000 | 86462–86462 / 86208–86208 |
| real-1000000 | cutadapt: gzip level 6 (1 CPU) | 0.106 / 0.117 | 9.400 / 8.550 | 0.000 / 0.000 | 118501947–118501947 / 117470202–117470202 |
| real-1000000 | cutadapt: plain output (4 CPUs) | 1.284 / 1.295 | 3.070 / 2.650 | 0.000 / 0.000 | 367627175–367627175 / 367627175–367627175 |
| synthetic-bam-100000 | samtools flagstat | 9.710 / 12.541 | 0.000 / 0.000 | 0.000 / 0.000 | 471–471 / 471–471 |
| synthetic-bam-100000 | samtools sort (32 MiB chunks) | 0.584 / 2.361 | 0.160 / 0.030 | 1.534 / 0.000 | 513111–513111 / 492460–492461 |
| synthetic-bam-100000 | UMI-tools dedup | 2.221 / 0.078 | 0.030 / 1.260 | 0.000 / 0.139 | 147134–147134 / 145842–145842 |
| synthetic-bam-1000000 | samtools flagstat | 10.709 / 26.582 | 0.090 / 0.030 | 0.000 / 0.000 | 475–475 / 475–475 |
| synthetic-bam-1000000 | samtools sort (32 MiB chunks) | 0.588 / 1.628 | 1.680 / 0.600 | 15.353 / 4.806 | 5138103–5138103 / 4932774–4932775 |
| synthetic-bam-1000000 | UMI-tools dedup | 2.206 / 0.195 | 0.440 / 5.090 | 0.000 / 1.396 | 1474676–1474676 / 1463412–1463412 |
| synthetic-10000000 | SeqKit fq2fa | 5.051 / 7.760 | 1.970 / 1.270 | 0.000 / 0.000 | 1710000000–1710000000 / 1710000000–1710000000 |
| synthetic-10000000 | fq lint | 1.546 / 5.104 | 6.440 / 1.940 | 0.000 / 0.000 | 785–785 / 1049–1049 |
| synthetic-bam-10000000 | samtools flagstat | 10.414 / 26.736 | 0.950 / 0.360 | 0.000 / 0.000 | 479–479 / 479–479 |
