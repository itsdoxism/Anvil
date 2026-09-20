mod deployment;
mod remote;

use anyhow::{bail, Context, Result};
use anvil_core::Repository;
use anvil_protocol::{AgentResponse, EntryKind};
use clap::{Parser, Subcommand};
use std::{
    fs,
    io::{self, Write},
    path::{Path, PathBuf},
};

#[derive(Debug, Parser)]
#[command(name = "anvil", version, about = "Local-first Minecraft server tooling for Linux")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum PluginCommand {
    /// Show the local deployment journal for one remote plugin JAR.
    History {
        name: String,
        plugin: String,
        #[arg(short = 'n', long, default_value_t = 20)]
        limit: usize,
    },
}

#[derive(Debug, Subcommand)]
enum Command {
    Init {
        #[arg(default_value = ".")]
        path: PathBuf,
    },
    Status {
        #[arg(default_value = ".")]
        path: PathBuf,
        #[arg(long)]
        json: bool,
    },
    Commit {
        #[arg(default_value = ".")]
        path: PathBuf,
        #[arg(short, long)]
        message: String,
    },
    Log {
        #[arg(default_value = ".")]
        path: PathBuf,
        #[arg(short = 'n', long, default_value_t = 20)]
        limit: usize,
    },
    Pair {
        name: String,
        endpoint: String,
        #[arg(long)]
        code: Option<String>,
    },
    Info {
        name: String,
        #[arg(long)]
        json: bool,
    },
    Players {
        name: String,
        #[arg(long)]
        json: bool,
    },
    /// List a remote server directory as a tree.
    Tree {
        name: String,
        #[arg(default_value = "")]
        path: String,
        #[arg(long, default_value_t = 2)]
        depth: usize,
    },
    /// Print a remote UTF-8 text file.
    Cat {
        name: String,
        path: String,
    },
    /// Download a remote file.
    Pull {
        name: String,
        remote_path: String,
        local_path: Option<PathBuf>,
    },
    /// Upload a local file. Existing remote content is backed up locally first.
    Push {
        name: String,
        local_path: PathBuf,
        remote_path: String,
        /// Skip the overwrite confirmation.
        #[arg(short = 'y', long)]
        yes: bool,
    },
    /// Deploy a plugin JAR and journal the previous/current versions locally.
    Deploy {
        name: String,
        jar: PathBuf,
        /// Override the remote JAR path. Defaults to plugins/<local-filename>.
        #[arg(long)]
        remote_path: Option<String>,
        #[arg(short = 'y', long)]
        yes: bool,
    },
    /// Restore the previous deployed JAR, or a specific local object by hash prefix.
    Rollback {
        name: String,
        plugin: String,
        #[arg(long)]
        to: Option<String>,
        #[arg(short = 'y', long)]
        yes: bool,
    },
    /// Plugin deployment history and related utilities.
    Plugin {
        #[command(subcommand)]
        command: PluginCommand,
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
                for path in status.added { println!("A  {path}"); }
                for path in status.modified { println!("M  {path}"); }
                for path in status.deleted { println!("D  {path}"); }
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
        Command::Pair { name, endpoint, code } => {
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
        Command::Tree { name, path, depth } => {
            let server = remote::load(&name)?;
            let label = if path.is_empty() { "/" } else { &path };
            println!("{label}");
            print_tree(&server, &path, "", depth)?;
        }
        Command::Cat { name, path } => {
            let server = remote::load(&name)?;
            let file = remote::read_file(&server, &path)?;
            let text = std::str::from_utf8(&file.bytes)
                .with_context(|| format!("{path} is not UTF-8 text; use anvil pull instead"))?;
            print!("{text}");
        }
        Command::Pull {
            name,
            remote_path,
            local_path,
        } => {
            let server = remote::load(&name)?;
            let file = remote::read_file(&server, &remote_path)?;
            let destination = local_path.unwrap_or_else(|| {
                Path::new(&remote_path)
                    .file_name()
                    .map(PathBuf::from)
                    .unwrap_or_else(|| PathBuf::from("download"))
            });
            if let Some(parent) = destination.parent() {
                if !parent.as_os_str().is_empty() {
                    fs::create_dir_all(parent)?;
                }
            }
            fs::write(&destination, &file.bytes)?;
            println!(
                "Pulled {} -> {} ({} bytes, {})",
                remote_path,
                destination.display(),
                file.bytes.len(),
                &file.sha256[..12]
            );
        }
        Command::Push {
            name,
            local_path,
            remote_path,
            yes,
        } => {
            let server = remote::load(&name)?;
            let bytes = fs::read(&local_path)
                .with_context(|| format!("failed to read {}", local_path.display()))?;
            let local_hash = remote::sha256_bytes(&bytes);

            if let Some(previous) = remote::try_read_file(&server, &remote_path)? {
                if previous.sha256 == local_hash {
                    println!("Already up to date: {remote_path}");
                    return Ok(());
                }

                let object = remote::store_backup(&server, &previous)?;
                println!(
                    "Backed up remote {} ({} -> {})",
                    remote_path,
                    &previous.sha256[..12],
                    object.display()
                );

                if !yes
                    && !confirm(&format!(
                        "Replace {} with {}? [y/N] ",
                        remote_path,
                        local_path.display()
                    ))?
                {
                    println!("Cancelled.");
                    return Ok(());
                }
            }

            let hash = remote::write_file(&server, &remote_path, &bytes)?;
            println!(
                "Pushed {} -> {} ({} bytes, {})",
                local_path.display(),
                remote_path,
                bytes.len(),
                &hash[..12]
            );
        }
        Command::Deploy {
            name,
            jar,
            remote_path,
            yes,
        } => {
            let server = remote::load(&name)?;
            let target = match remote_path {
                Some(path) => path,
                None => deployment::default_remote_path(&jar)?,
            };

            if !yes
                && !confirm(&format!(
                    "Deploy {} -> {}:{}? [y/N] ",
                    jar.display(),
                    name,
                    target
                ))?
            {
                println!("Cancelled.");
                return Ok(());
            }

            let record = deployment::deploy(&server, &jar, &target)?;
            println!("Deployed {} -> {}:{}", jar.display(), name, target);
            match &record.from_sha256 {
                Some(previous) => println!(
                    "{} -> {}",
                    deployment::short(previous),
                    deployment::short(&record.to_sha256)
                ),
                None => println!("new -> {}", deployment::short(&record.to_sha256)),
            }
            println!("Previous and deployed JAR bytes are stored locally.");
        }
        Command::Rollback {
            name,
            plugin,
            to,
            yes,
        } => {
            let server = remote::load(&name)?;
            let target = deployment::normalize_plugin_path(&plugin);

            if !yes
                && !confirm(&format!(
                    "Rollback {}:{}{}? [y/N] ",
                    name,
                    target,
                    to.as_ref()
                        .map(|hash| format!(" to {hash}"))
                        .unwrap_or_default()
                ))?
            {
                println!("Cancelled.");
                return Ok(());
            }

            let record = deployment::rollback(&server, &target, to.as_deref())?;
            println!(
                "Rolled back {}:{} {} -> {}",
                name,
                target,
                record
                    .from_sha256
                    .as_deref()
                    .map(deployment::short)
                    .unwrap_or("new"),
                deployment::short(&record.to_sha256)
            );
        }
        Command::Plugin { command } => match command {
            PluginCommand::History {
                name,
                plugin,
                limit,
            } => {
                let server = remote::load(&name)?;
                let target = deployment::normalize_plugin_path(&plugin);
                let history = deployment::history(&server, &target)?;

                if history.is_empty() {
                    println!("no deployment history for {}:{}", name, target);
                } else {
                    for record in history.into_iter().rev().take(limit) {
                        let action = match record.action {
                            deployment::DeploymentAction::Deploy => "deploy",
                            deployment::DeploymentAction::Rollback => "rollback",
                        };
                        let from = record
                            .from_sha256
                            .as_deref()
                            .map(deployment::short)
                            .unwrap_or("new");
                        println!(
                            "{}  {:8}  {} -> {}",
                            record.timestamp.to_rfc3339(),
                            action,
                            from,
                            deployment::short(&record.to_sha256)
                        );
                    }
                }
            }
        },
    }
    Ok(())
}

fn print_tree(
    server: &remote::RemoteServer,
    path: &str,
    prefix: &str,
    depth: usize,
) -> Result<()> {
    if depth == 0 {
        return Ok(());
    }
    let entries = remote::list_dir(server, path)?;
    let len = entries.len();
    for (index, entry) in entries.into_iter().enumerate() {
        let last = index + 1 == len;
        let connector = if last { "└── " } else { "├── " };
        let suffix = match entry.kind {
            EntryKind::Directory => "/",
            EntryKind::Symlink => "@",
            EntryKind::File => "",
        };
        println!("{prefix}{connector}{}{suffix}", entry.name);

        if entry.kind == EntryKind::Directory {
            let child_prefix = format!("{prefix}{}", if last { "    " } else { "│   " });
            print_tree(server, &entry.path, &child_prefix, depth - 1)?;
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

fn confirm(label: &str) -> Result<bool> {
    let value = prompt(label)?;
    Ok(matches!(value.to_ascii_lowercase().as_str(), "y" | "yes"))
}

fn short(id: &str) -> &str {
    &id[..id.len().min(10)]
}
