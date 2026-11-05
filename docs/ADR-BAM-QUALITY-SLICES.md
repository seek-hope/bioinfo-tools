# Borrowed quality slices for the existing BAM encoder

The BAM performance work adds a direct edge from `rust-bam` to **bstr 1.13.1**,
which is already locked and used transitively by noodles. Its source is crates.io
(`registry+https://github.com/rust-lang/crates.io-index`), checksum
`6bb31b46c14244e20ee9984b11bf5c992b91fb6939fea616e3512c8baecdbe5f`.
Default features are disabled; `std` is enabled. No package version or C library
is introduced. Build with `--locked`.

The reason for the direct edge is the `BStr` return type required to implement
noodles' alignment-record trait. A private borrowed adapter forwards the original
`RecordBuf` fields and exposes its quality byte slice to the existing noodles BAM
encoder. `RecordBuf` already exposes its sequence slice, but its quality accessor
currently falls back to a boxed, per-value iterator. The adapter removes that
iterator and its allocation without replacing the BAM encoder.

The selected noodles path still checks every quality byte against the legal
0–93 range, handles missing qualities, and checks sequence/quality lengths. Field
ordering and all other encoding/decoding stay with noodles. Its BAM writer encodes
a complete record into a private buffer before emitting the frame, so a failure
must leave identical output even though the slice path checks before appending.
Differential tests cover successful bytes, error kinds/messages, partial output,
and successful writes following a failed record.

The dependency remains statically linked pure Rust, adds no service or executable,
and names an API type already present in the build. Compare the resolved bstr
feature graph before and after the change; record any expansion instead of
assuming this edge is free. The adapter's runtime state is one borrowed reference.
Whole-record encoding buffers retain their existing largest-record memory costs.
