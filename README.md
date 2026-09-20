# Anvil

**Local-first version control and remote tooling for Minecraft servers on Linux.**

Anvil is built for server owners and developers who would rather open a terminal than a web panel. The Linux CLI owns history, snapshots and backups; a small Paper/Purpur agent exposes server files and runtime data over a direct connection.

## Local versioning

```bash
cargo run -p anvil-cli -- init ./server
cargo run -p anvil-cli -- status ./server
cargo run -p anvil-cli -- commit ./server -m "before plugin update"
cargo run -p anvil-cli -- log ./server
```

Repositories use a SHA-256 content-addressed object store under `.anvil/`, so unchanged files are not duplicated between commits.

## Remote milestone

The first Anvil Agent milestone now supports pairing plus live server/player information:

```bash
anvil pair mellow 192.168.1.20:45920
anvil info mellow
anvil players mellow
anvil players mellow --json
```

The current v0 transport is **development-only and not encrypted yet**. The agent binds to `127.0.0.1` by default. Use a trusted LAN/VPN for remote testing until authenticated encryption lands.

The next remote milestone adds filesystem operations (`tree`, `pull`, `push`) and safe plugin JAR deployment/rollback.

See [`docs/ARCHITECTURE.md`](docs/ARCHITECTURE.md) and [`agent/README.md`](agent/README.md).
