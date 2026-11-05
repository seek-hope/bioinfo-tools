# BAM sort header preservation

Coordinate sorting previously reconstructed the parsed SAM header from its
`@HD`, reference, read-group and program maps. It omitted every `@CO` comment.
Changing only `SO` also left stale `GO` and `SS` declarations describing the
input's previous order.

The retained local reproducer is `target/surpass-d3d8edc/header-probe/`. On the same
input, the compact candidate before this correction printed:

```text
@HD VN:1.6 SO:coordinate SS:queryname:natural GO:query ZZ:keep
```

It lost both input comments. Pinned samtools 1.24 printed:

```text
@HD VN:1.6 SO:coordinate ZZ:keep
...
@CO first comment
@CO second<TAB>comment
```

The example above uses spaces for header-field separators for readability; raw
stdout, stderr, input and commands are retained. Both tools exited zero. This is
a native correctness bug, not an approved upstream deviation.

`header_with_coordinate_sort` now updates the owned header's `@HD` in place,
sets `SO:coordinate`, and removes `GO` and `SS` while preserving the order of the
remaining fields. It retains comments, references, read groups and programs.
Absent `@HD` still receives the existing `VN:1.6` default. Record encoding sees the
same reference map, including when compact chunks were encoded before the final
header was constructed.

The regression checks the complete serialized header, two ordered comments
including an embedded tab, custom `@HD` metadata, and unchanged reference,
read-group and program maps. The component release suite and all-target Clippy
passed in `header-tests.log` and `header-clippy.log`.

The sealed CLI comparison in `header-validation/report.json` passed **144 exact
full-header and full-record comparisons** against samtools. It includes query-name
and coordinate subsort metadata, absent order, absent `@HD`, absent comments,
empty alignments, UTF-8 comments, SAM/BAM input and output, one/four workers,
in-memory sorting, spills and multi-level merging. samtools ran with `--no-PG`, so
these comparisons did not remove any header fields or alignment fields. Complete
input/output/command artifacts remain in that directory, and the original loss
reproducer is retained separately.

`header-sealed/` and `header-source/` preserve the corrected runtime source and
validation infrastructure. `header-gates.json` records fresh successful runs of
both required repository gates, followed by the sealed samtools differential suite
(20/20). Those gates include the new benchmark resource/batch controls. This is
correctness evidence; formal performance acceptance and comprehensive superiority
remain open.
