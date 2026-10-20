# Repository history

The workspace originally contained two independent Git repositories:

| Archived history | Original remote |
| --- | --- |
| `../bioinfo-tools-historical/rust-cutadapt.git` | `https://github.com/seek-hope/rust-cutadapt.git` |
| `../bioinfo-tools-historical/rust-umitools.git` | `https://github.com/seek-hope/rust-umitools.git` |

They were archived when the workspace became a monorepo. Their final local
commits were:

```text
rust-cutadapt  2301bb2 refactor: adopt shared contracts and FASTX layer
rust-umitools  6b89b0d refactor: adopt shared contracts and FASTX layer
```

The archived Git directories are read-only historical records. Build and test
from this monorepo, not from the archives.
