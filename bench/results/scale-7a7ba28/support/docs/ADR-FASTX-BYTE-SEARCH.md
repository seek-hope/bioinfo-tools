# Direct byte-search dependency for FASTX streaming

The larger-input performance work makes `memchr` 2.8.3 a direct dependency of
`rust-fx`. The source is crates.io (`registry+https://github.com/rust-lang/crates.io-index`),
with the existing version and checksum in `Cargo.lock`. This package was already
present through `regex`; no package or system library is added to the dependency
closure. Default features are disabled and `std` is enabled, matching the former
package-only feature selection (`alloc`, `std`).

The reason is to locate FASTQ newline boundaries directly in the existing read
buffer, so complete records can be inspected or written without copying all four
fields into owned vectors. The standard library does not expose its optimized
byte-search primitive. Reusing the pinned implementation avoids a new SIMD or
unsafe implementation in this repository.

The API is statically linked, requires no runtime service, and retains portable
runtime CPU feature detection. The borrowed path applies only to complete
four-line records in a buffer. Boundary-spanning records, FASTA, wrapped FASTQ,
and strict parse errors retain the existing parser and diagnostics. Build with
`--locked`; benchmark records must include the resulting executable hashes.
