| Dataset | Command | Before s [min–max] | After s [min–max] | Upstream s [min–max] | Paired before/after | Max RSS MiB before/after/upstream |
| --- | --- | ---: | ---: | ---: | ---: | ---: |
| real-1000000 | stats -a -T | 1.972 [1.971–1.974] | 0.621 [0.620–0.626] | 0.501 [0.499–0.501] | 3.17× | 5.3 / 5.2 / 36.3 |
| real-1000000 | fq2fa | 0.200 [0.199–0.201] | 0.171 [0.167–0.172] | 0.251 [0.249–0.257] | 1.17× | 4.8 / 4.8 / 35.9 |
| real-1000000 | lint | 0.529 [0.528–0.532] | 0.236 [0.235–0.236] | 0.197 [0.197–0.198] | 2.24× | 149.7 / 4.8 / 4.4 |
| real-1000000 | FastQC* | 8.025 [8.018–8.046] | 4.479 [4.465–4.511] | 6.171 [6.166–6.175] | 1.79× | 92.0 / 92.1 / 307.8 |
| synthetic-1000000 | stats -a -T | 1.956 [1.955–1.959] | 0.615 [0.613–0.617] | 0.373 [0.370–0.375] | 3.18× | 5.2 / 5.2 / 36.6 |
| synthetic-1000000 | fq2fa | 0.185 [0.183–0.189] | 0.158 [0.154–0.159] | 0.133 [0.131–0.141] | 1.17× | 4.9 / 4.7 / 32.6 |
| synthetic-1000000 | lint | 0.483 [0.481–0.496] | 0.222 [0.222–0.223] | 0.189 [0.188–0.190] | 2.18× | 107.8 / 4.9 / 4.4 |
| synthetic-1000000 | FastQC* | 7.987 [7.978–8.001] | 4.401 [4.395–4.443] | 5.174 [5.166–6.175] | 1.81× | 110.0 / 110.1 / 296.3 |
| synthetic-bam-1000000 | flagstat | 0.083 [0.083–0.084] | 0.070 [0.070–0.070] | 0.037 [0.036–0.037] | 1.20× | 4.0 / 4.1 / 3.9 |
| synthetic-bam-1000000 | sort (32 MiB) | 1.406 [1.391–1.453] | 1.246 [1.231–1.300] | 0.533 [0.531–0.540] | 1.13× | 43.8 / 43.3 / 53.0 |
| synthetic-10000000 | fq2fa | 1.843 [1.771–1.864] | 1.470 [1.445–1.503] | 1.197 [1.164–1.263] | 1.25× | 4.9 / 4.8 / 36.7 |
| synthetic-10000000 | lint | 5.614 [5.583–5.632] | 2.140 [2.131–2.147] | 1.789 [1.782–1.793] | 2.62× | 829.0 / 4.9 / 4.4 |
| synthetic-bam-10000000 | flagstat | 0.815 [0.812–0.822] | 0.703 [0.702–0.718] | 0.319 [0.319–0.323] | 1.16× | 4.0 / 3.9 / 3.8 |

*FastQC before/after compares identical native report bundles. Its upstream values are descriptive: missing native plots and unresolved statistical differences prevent an upstream speed ratio or whole-report equivalence claim.

Ratios above one favor the optimized native executable. Ratios are medians within matched blocks, not ratios of separate medians. Min–max values are observed ranges, not confidence intervals. RSS is the maximum GNU time observation, not simultaneous process-tree total memory.
