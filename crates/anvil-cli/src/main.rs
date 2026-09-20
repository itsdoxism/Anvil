use anyhow::{Context, Result};
use anvil_core::Repository;
use clap::{Parser, Subcommand};
use std::path::PathBuf;

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
    }
    Ok(())
}

fn short(id: &str) -> &str {
    &id[..id.len().min(10)]
}
