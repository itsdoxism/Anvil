# Anvil Agent

Paper/Purpur-side bridge for Anvil.

## Build

Current Paper development targets Java 25 / API 26.2.

```bash
cd agent
gradle build
```

The JAR is produced under `agent/build/libs/`.

## Development pairing

The current v0 transport is deliberately marked as a development transport: it authenticates with a one-time pair code and persistent random token, but it is **not encrypted yet**. The default bind address is therefore loopback-only.

For testing across a trusted LAN/VPN, edit `plugins/AnvilAgent/config.yml`:

```yaml
bind: "0.0.0.0"
port: 45920
```

Then run `/anvil pair` from the server console and pair from Linux:

```bash
anvil pair mellow 192.168.1.20:45920
anvil info mellow
anvil players mellow
```

Do not expose the v0 transport directly to the public Internet. Authenticated encryption / certificate pinning is the next transport milestone.
