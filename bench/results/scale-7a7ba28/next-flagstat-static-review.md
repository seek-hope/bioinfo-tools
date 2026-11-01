# Flagstat follow-up: static review only

Read-only review during the frozen formal measurement. No commands, tests,
profiles, source edits or performance experiment were run by the reviewer.

The current path already reuses one RecordBuf in commands.rs::flagstat, calls
AlignmentReader::read_record -> noodles BAM read_record_buf -> full decode, then
reads scalar fields and borrows cached reference names. RecordBuf accessors,
MappingQuality::get, Vec::get and String::as_str do not allocate. FlagStat::add
uses integer increments, flag tests and a gated string comparison. Final report
formatting occurs after EOF. No worthwhile post-decode allocation/clone/conversion
candidate was found in this scope.

Possible deferred-name lookups or equality-class IDs would only be CPU
microoptimizations and need profiling evidence first. Any equality-class design
must preserve lossy UTF-8 name collisions, unknown positive IDs, absent IDs and
the current different-chromosome counting conditions. Raw reference-ID inequality
is not equivalent to the current string comparison. Full decode and diagnostic
order remain required; raw bam::Record access is not a replacement.

Future checks if this path changes: MAPQ missing/4/5 and all relevant flag
combinations; lossy-name collisions and IDs beyond the header dictionary;
multiply malformed records and exact error precedence; no flagstat report or
output-file overwrite on read error; SAM parsing and CRAM rejection for file and
stdin. Some introductory stats.rs comments differ from its implemented counting
conditions, so use implementation and oracle evidence when reviewing them.

Anchors at review: rust-bam/src/commands.rs:1562 and :33;
rust-bam/src/io_util.rs:151; rust-bam/src/stats.rs:100;
rust-bam/src/main.rs:662. Pinned noodles-bam0.97.0 read_record_buf/decoder and
noodles-sam0.92.0 RecordBuf accessors were inspected.
