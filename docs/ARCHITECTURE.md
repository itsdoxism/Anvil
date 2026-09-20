# Architecture

Anvil is intentionally local-first and Linux-first.

## Components

### `anvil` CLI

Runs on the operator's Linux machine. It owns the durable state:

- content-addressed objects
- commits and refs
- diffs and snapshots
- remote server identities and keys (later)
- deployment/rollback history (later)

The CLI must remain usable from Bash, Zsh, Fish, Nushell and other shells. Human output is the default; automation-oriented commands should expose `--json`.

### Anvil Agent (planned)

A small Paper/Purpur plugin installed on the Minecraft server. It should be a deliberately narrow bridge rather than a second control plane.

Responsibilities:

- expose server-root filesystem operations within an explicit sandbox
- expose Paper player/server metadata
- stream logs/events
- accept authenticated direct connections
- verify file uploads before atomic replacement

It should **not** execute arbitrary Linux shell commands.

## Connection model

The intended default is direct TCP with authenticated encryption:

```text
Linux workstation                         Minecraft host
+--------------------+       TLS          +-----------------------+
| anvil CLI          | <----------------> | Anvil Agent (Paper)   |
| history / objects  |                    | filesystem / API      |
+--------------------+                    +-----------------------+
```

The Minecraft host listens on a dedicated Anvil port. Pairing is one-time; the final design will use persistent cryptographic identities rather than reusable passwords.

## Local repository format (v1)

```text
server/
├── .anvilignore
├── .anvil/
│   ├── repo.json
│   ├── refs/HEAD
│   ├── commits/<commit-id>.json
│   └── objects/ab/cdef...
└── ... mirrored/server files ...
```

Objects are addressed by SHA-256. Commits reference objects instead of copying entire snapshots, so identical files are stored once.

## Safety principles

1. Every deploy captures the previous remote object locally before replacement.
2. Uploads land in a temporary file, are checksum-verified, then atomically renamed.
3. Destructive writes require explicit user approval unless an explicit non-interactive flag is supplied.
4. The agent is sandboxed to the Minecraft server root by default.
5. Local history remains useful even when the remote server is offline.
