use crate::remote::{self, RemoteServer};
use anyhow::{bail, Context, Result};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::{
    fs::{self, OpenOptions},
    io::{BufRead, BufReader, Write},
    path::{Path, PathBuf},
};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeploymentRecord {
    pub timestamp: DateTime<Utc>,
    pub action: DeploymentAction,
    pub remote_path: String,
    pub from_sha256: Option<String>,
    pub to_sha256: String,
    pub source: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DeploymentAction {
    Deploy,
    Rollback,
}

pub fn deploy(
    server: &RemoteServer,
    local_jar: &Path,
    remote_path: &str,
) -> Result<DeploymentRecord> {
    ensure_jar(local_jar)?;
    let bytes = fs::read(local_jar)
        .with_context(|| format!("failed to read {}", local_jar.display()))?;
    let (to_sha256, _) = remote::store_object(&bytes)?;

    let previous = remote::try_read_file(server, remote_path)?;
    let from_sha256 = if let Some(previous) = previous {
        if previous.sha256 == to_sha256 {
            bail!("remote plugin is already identical to {}", local_jar.display());
        }
        remote::store_object(&previous.bytes)?;
        Some(previous.sha256)
    } else {
        None
    };

    remote::write_file(server, remote_path, &bytes)?;

    let record = DeploymentRecord {
        timestamp: Utc::now(),
        action: DeploymentAction::Deploy,
        remote_path: remote_path.to_owned(),
        from_sha256,
        to_sha256,
        source: Some(local_jar.display().to_string()),
    };
    append_record(server, &record)?;
    Ok(record)
}

pub fn rollback(
    server: &RemoteServer,
    remote_path: &str,
    requested: Option<&str>,
) -> Result<DeploymentRecord> {
    let current = remote::read_file(server, remote_path)?;
    remote::store_object(&current.bytes)?;

    let history = history(server, remote_path)?;
    let target_sha = match requested {
        Some(prefix) => resolve_prefix(&history, prefix)?,
        None => history
            .iter()
            .rev()
            .find(|record| record.to_sha256 == current.sha256)
            .and_then(|record| record.from_sha256.clone())
            .ok_or_else(|| anyhow::anyhow!(
                "no rollback target found for current remote hash {}",
                short(&current.sha256)
            ))?,
    };

    if target_sha == current.sha256 {
        bail!("rollback target is already active");
    }

    let bytes = remote::load_object(&target_sha)?;
    let verified = remote::sha256_bytes(&bytes);
    if verified != target_sha {
        bail!("local rollback object checksum mismatch");
    }

    remote::write_file(server, remote_path, &bytes)?;

    let record = DeploymentRecord {
        timestamp: Utc::now(),
        action: DeploymentAction::Rollback,
        remote_path: remote_path.to_owned(),
        from_sha256: Some(current.sha256),
        to_sha256: target_sha,
        source: None,
    };
    append_record(server, &record)?;
    Ok(record)
}

pub fn history(server: &RemoteServer, remote_path: &str) -> Result<Vec<DeploymentRecord>> {
    let path = journal_path(server, remote_path)?;
    if !path.exists() {
        return Ok(Vec::new());
    }

    let file = fs::File::open(&path)?;
    let mut out = Vec::new();
    for line in BufReader::new(file).lines() {
        let line = line?;
        if line.trim().is_empty() {
            continue;
        }
        out.push(
            serde_json::from_str(&line)
                .with_context(|| format!("invalid deployment journal {}", path.display()))?,
        );
    }
    Ok(out)
}

pub fn default_remote_path(local_jar: &Path) -> Result<String> {
    ensure_jar(local_jar)?;
    let name = local_jar
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| anyhow::anyhow!("invalid JAR file name"))?;
    Ok(format!("plugins/{name}"))
}

pub fn normalize_plugin_path(value: &str) -> String {
    if value.contains('/') {
        value.to_owned()
    } else {
        format!("plugins/{value}")
    }
}

pub fn short(value: &str) -> &str {
    &value[..value.len().min(12)]
}

fn append_record(server: &RemoteServer, record: &DeploymentRecord) -> Result<()> {
    let path = journal_path(server, &record.remote_path)?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }

    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)?;
    serde_json::to_writer(&mut file, record)?;
    file.write_all(b"\n")?;
    file.sync_all()?;
    Ok(())
}

fn journal_path(server: &RemoteServer, remote_path: &str) -> Result<PathBuf> {
    let key = remote::sha256_bytes(remote_path.as_bytes());
    Ok(remote::data_root()?
        .join("deployments")
        .join(&server.name)
        .join(format!("{key}.jsonl")))
}

fn resolve_prefix(history: &[DeploymentRecord], prefix: &str) -> Result<String> {
    if prefix.len() < 6 {
        bail!("rollback hash prefix must contain at least 6 characters");
    }

    let mut matches = history
        .iter()
        .flat_map(|record| {
            record
                .from_sha256
                .iter()
                .chain(std::iter::once(&record.to_sha256))
        })
        .filter(|hash| hash.starts_with(prefix))
        .cloned()
        .collect::<Vec<_>>();
    matches.sort();
    matches.dedup();

    match matches.as_slice() {
        [one] => Ok(one.clone()),
        [] => bail!("no deployment object matches '{prefix}'"),
        _ => bail!("rollback hash prefix '{prefix}' is ambiguous"),
    }
}

fn ensure_jar(path: &Path) -> Result<()> {
    let is_jar = path
        .extension()
        .and_then(|ext| ext.to_str())
        .is_some_and(|ext| ext.eq_ignore_ascii_case("jar"));
    if !is_jar {
        bail!("expected a .jar file: {}", path.display());
    }
    Ok(())
}
