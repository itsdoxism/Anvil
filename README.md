# Anvil

**Local-first version control and remote tooling for Minecraft servers on Linux.**

Anvil is built for server owners and developers who would rather open a terminal than a web panel. The Linux CLI owns history, snapshots and backups; a small Paper/Purpur agent exposes server files and runtime data over a direct connection.

## Local versioning

```bash
anvil init ./server
anvil status ./server
anvil commit ./server -m "before plugin update"
anvil log ./server
```

Repositories use a SHA-256 content-addressed object store under `.anvil/`, so unchanged files are not duplicated between commits.

## Remote server

Pair once:

```bash
anvil pair mellow 192.168.1.20:45920
```

Then work from any shell:

```bash
anvil info mellow
anvil players mellow
anvil tree mellow plugins --depth 3
anvil cat mellow plugins/MellowGuard/config.yml
anvil pull mellow logs/latest.log ./latest.log
anvil push mellow ./config.yml plugins/MellowGuard/config.yml
```

When `push` replaces an existing remote file, Anvil first downloads the old bytes into the local content-addressed backup store and asks for confirmation. Uploads are SHA-256 verified and installed through a temporary file plus atomic replace where supported.

The agent sandboxes file operations to the Minecraft server directory and rejects absolute paths, traversal outside the root, and symlink traversal. Single transfers are capped by `max-transfer-bytes`.

> The current v0 transport is development-only and **not encrypted yet**. The agent binds to `127.0.0.1` by default. Use a trusted LAN/VPN until authenticated encryption lands.

## Plugin deployment

Anvil keeps plugin deployment history locally and stores JAR bytes by SHA-256:

```bash
anvil deploy mellow ./build/libs/MellowGuard.jar
anvil plugin history mellow MellowGuard
anvil rollback mellow MellowGuard

# restore a specific version from the local object store
anvil rollback mellow MellowGuard --to 8f21c1
```

By default, `deploy` targets `plugins/<local-jar-name>`. Use `--remote-path` when the remote filename differs.

Before replacing a remote JAR, Anvil stores the old JAR locally. The newly deployed JAR is stored locally too, so rollback does not depend on the remote host retaining older files.

Anvil currently replaces the JAR on disk only; it does not hot-reload Paper plugins. Restart or otherwise reload the server using your normal server workflow.

Next: encrypted transport, richer deployment health checks, and server log streaming.

See [`docs/ARCHITECTURE.md`](docs/ARCHITECTURE.md) and [`agent/README.md`](agent/README.md).
