mod remote;

use anyhow::{bail, Context, Result};
use anvil_core::Repository;
use anvil_protocol::AgentResponse;
use clap::{Parser, Subcommand};
use std::{
    io::{self, Write},
    path::PathBuf,
};

#[derive(Debug, Parser)]
#[command(name = "anvil", version, about = "Local-first Minecraft server tooling for Linux")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Initialize an Anvil repository.
    Init {
        #[arg(default_value = ".")]
        path: PathBuf,
    },
    /// Show files changed since the latest commit.
    Status {
        #[arg(default_value = ".")]
        path: PathBuf,
        /// Emit machine-readable JSON.
        #[arg(long)]
        json: bool,
    },
    /// Snapshot the current filesystem state.
    Commit {
        #[arg(default_value = ".")]
        path: PathBuf,
        #[arg(short, long)]
        message: String,
    },
    /// Show local Anvil commit history.
    Log {
        #[arg(default_value = ".")]
        path: PathBuf,
        #[arg(short = 'n', long, default_value_t = 20)]
        limit: usize,
    },
    /// Pair this Linux machine with an Anvil Agent.
    Pair {
        /// Local alias, for example "mellow".
        name: String,
        /// Agent endpoint, for example "mellowsmp.fun:45920".
        endpoint: String,
        /// One-time code printed by the agent. If omitted, Anvil prompts for it.
        #[arg(long)]
        code: Option<String>,
    },
    /// Show remote Minecraft server information.
    Info {
        name: String,
        #[arg(long)]
        json: bool,
    },
    /// Show online players from a paired server.
    Players {
        name: String,
        #[arg(long)]
        json: bool,
    },
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    match cli.command {
        Command::Init { path } => {
            let repo = Repository::init(&path).context("failed to initialize repository")?;
            println!("Initialized Anvil repository in {}", repo.root().display());
        }
        Command::Status { path, json } => {
            let repo = Repository::open(&path)?;
            let status = repo.status()?;
            if json {
                let value = serde_json::json!({
                    "clean": status.is_clean(),
                    "added": status.added,
                    "modified": status.modified,
                    "deleted": status.deleted,
                    "unchanged": status.unchanged,
                });
                println!("{}", serde_json::to_string_pretty(&value)?);
            } else if status.is_clean() {
                println!("clean");
            } else {
                for path in status.added {
                    println!("A  {path}");
                }
                for path in status.modified {
                    println!("M  {path}");
                }
                for path in status.deleted {
                    println!("D  {path}");
                }
            }
        }
        Command::Commit { path, message } => {
            let repo = Repository::open(&path)?;
            let commit = repo.commit(message)?;
            println!("[{}] {}", short(&commit.id), commit.message);
            println!("{} files", commit.files.len());
        }
        Command::Log { path, limit } => {
            let repo = Repository::open(&path)?;
            for commit in repo.log()?.into_iter().take(limit) {
                println!("commit {}", commit.id);
                println!("Date:   {}", commit.created_at.to_rfc3339());
                println!();
                println!("    {}", commit.message);
                println!();
            }
        }
        Command::Pair {
            name,
            endpoint,
            code,
        } => {
            let code = code.unwrap_or(prompt("Pair code: ")?);
            let paired = remote::pair(&name, &endpoint, &code)?;
            println!("Paired '{}' with {}", paired.name, paired.endpoint);
            println!("Server ID: {}", paired.server_id);
        }
        Command::Info { name, json } => {
            let server = remote::load(&name)?;
            match remote::request(&server, "SERVER_INFO")? {
                response @ AgentResponse::ServerInfo { .. } => {
                    let info = response.into_server_info().expect("matched server info");
                    if json {
                        println!("{}", serde_json::to_string_pretty(&info)?);
                    } else {
                        println!("{} ({})", name, info.server_id);
                        println!("Minecraft: {}", info.minecraft_version);
                        println!("Server:    {}", info.implementation);
                        println!("Players:   {}/{}", info.online_players, info.max_players);
                    }
                }
                AgentResponse::Error { code, message } => bail!("{code}: {message}"),
                other => bail!("unexpected agent response: {other:?}"),
            }
        }
        Command::Players { name, json } => {
            let server = remote::load(&name)?;
            match remote::request(&server, "PLAYERS")? {
                AgentResponse::Players { players } => {
                    if json {
                        println!("{}", serde_json::to_string_pretty(&players)?);
                    } else if players.is_empty() {
                        println!("no players online");
                    } else {
                        println!("{:<18} {:>6}  {:<16} {}", "NAME", "PING", "WORLD", "MODE");
                        for player in players {
                            println!(
                                "{:<18} {:>4}ms  {:<16} {}",
                                player.name, player.ping_ms, player.world, player.game_mode
                            );
                        }
                    }
                }
                AgentResponse::Error { code, message } => bail!("{code}: {message}"),
                other => bail!("unexpected agent response: {other:?}"),
            }
        }
    }
    Ok(())
}

fn prompt(label: &str) -> Result<String> {
    print!("{label}");
    io::stdout().flush()?;
    let mut value = String::new();
    io::stdin().read_line(&mut value)?;
    Ok(value.trim().to_owned())
}

fn short(id: &str) -> &str {
    &id[..id.len().min(10)]
}
