# Anvil Agent

Paper/Purpur-side bridge for Anvil.

## Build

```bash
cd agent
gradle build
```

The JAR is produced under `agent/build/libs/`.

## Development pairing

The current v0 transport authenticates with a one-time pair code and persistent random token, but is **not encrypted yet**. The default bind address is loopback-only.

For testing across a trusted LAN/VPN, edit `plugins/AnvilAgent/config.yml`:

```yaml
bind: "0.0.0.0"
port: 45920
max-transfer-bytes: 134217728
```

Then run `/anvil pair` from the server console and pair from Linux:

```bash
anvil pair mellow 192.168.1.20:45920
anvil info mellow
anvil players mellow
anvil tree mellow plugins
anvil pull mellow logs/latest.log
```

Filesystem operations are rooted at the Minecraft server working directory. Absolute paths, traversal outside the root, and symlink traversal are rejected.

Do not expose the v0 transport directly to the public Internet. Authenticated encryption / certificate pinning is the next transport milestone.
