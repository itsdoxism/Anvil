# Anvil

**Local-first version control and remote tooling for Minecraft servers on Linux.**

Anvil is being built for server owners and developers who would rather open a terminal than a web panel. The local CLI owns history, snapshots and backups; a small Paper/Purpur agent will later expose server files and runtime data over a direct, authenticated connection.

## Current state

The first milestone implements the local versioning core:

```bash
cargo run -p anvil-cli -- init ./server
cargo run -p anvil-cli -- status ./server
cargo run -p anvil-cli -- commit ./server -m "before plugin update"
cargo run -p anvil-cli -- log ./server
```

A repository stores metadata under `.anvil/` and file contents in a SHA-256 content-addressed object store, so unchanged files are not duplicated between commits.

## Direction

Planned next layers:

- direct Linux CLI ↔ Paper/Purpur agent pairing
- remote `pull`, `push`, `tree`, `cat` and `$EDITOR` workflows
- safe plugin JAR deployment with automatic local backup and rollback
- live logs, server info and player info
- agent-friendly JSON output and explicit approval gates for destructive writes

See [`docs/ARCHITECTURE.md`](docs/ARCHITECTURE.md).
