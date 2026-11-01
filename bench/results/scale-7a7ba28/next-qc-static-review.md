# Next FastQC closure: static leads, not acceptance evidence

Read-only helper review; no probes/tests/builds were run for these leads.
Do not change the sealed benchmark implementation while its measurements run.
Pinned source: FastQC0.13.0, da2674c2687d1e3296b34a696421b33b93cc8123.

1. Upstream PerSequenceGCContent.java:174,218 truncates150bp reads to their first
   100bp, explicitly to limit cached GC models. Native stats.rs:391,428 uses all
   150bp. This is a deliberate upstream approximation, not an established defect.
   Probe A^100 G^50 at150bp, then A^150, with valid constant qualities. Source
   predictions: upstream bins0=1,1=.5 for both; native first input33=.5,34=1/3,
   second input0=.5. GC model weighting itself is non-unit-normalized in both;
   do not replace it with unit weights without evidence and a declared deviation.
2. Upstream OverRepresentedSeqs.java:55,169 freezes new-prefix admission after
   100000 distinct prefixes but keeps counting known ones. Native stats.rs:460
   keeps all prefixes. DuplicationLevel.java:160 extrapolates each multiplicity
   class using m_d/(1-p0), p0=product((N-i-d)/(N-i),i=0..K-1); native modules.rs:701
   uses exhaustive counts. Probe100000 collision-free50bp prefixes padded to
   150bp, then two copies of one new prefix. Upstream predicts100% deduplicated
   and no doubletons; native predicts100*100001/100002% and two reads in bin2.
   Existing1M mismatch attribution still needs empirical input/order checks.
3. Native modules.rs:737 puts count50 in >50. Upstream DuplicationLevel.java:115
   uses dupLevel=count-1 and >49, placing counts10..50 in >10,51..100 in >50.
   Probe50 identical150bp reads. This is a concrete native boundary discrepancy;
   behavioral validation is pending.
4. Upstream DuplicationLevel.java:169 shortcut uses N-m_d<K, whereas its stated
   impossibility rationale depends on N-d<K. Probe100000 distinct prefixes,
   one repeat of the first, then one new prefix: K100000,N100002,d1,m1=99999.
   Shortcut99999 differs from the unshortened inclusion formula100000.99998.
   This establishes a formula discrepancy, not unbiased estimation or an
   automatically approved upstream correction: the sampling/stopping assumptions
   and observed behavior need independent analysis first. The0.01 cutoff at
   line181 is explicitly approximate and separately bounds its omitted correction.

Future probes must preserve fixtures, retain raw upstream/native results, and
separate intentional approximation, native defects and independently proved
upstream defects. No status or exception is changed by this note.
