use anyhow::{anyhow, bail, Context, Result};
use anvil_protocol::AgentResponse;
use serde::{Deserialize, Serialize};
use std::{
    env,
    fs,
    io::{BufRead, BufReader, Write},
    net::TcpStream,
    path::PathBuf,
    time::Duration,
};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RemoteServer {
    pub name: String,
    pub endpoint: String,
    pub server_id: String,
    pub token: String,
}

pub fn pair(name: &str, endpoint: &str, code: &str) -> Result<RemoteServer> {
    validate_name(name)?;
    let response = exchange(endpoint, &format!("PAIR {}", code.trim()))?;
    match response {
        AgentResponse::Paired { server_id, token } => {
            let remote = RemoteServer {
                name: name.to_owned(),
                endpoint: endpoint.to_owned(),
                server_id,
                token,
            };
            save(&remote)?;
            Ok(remote)
        }
        AgentResponse::Error { code, message } => bail!("{code}: {message}"),
        other => bail!("unexpected agent response: {other:?}"),
    }
}

pub fn request(remote: &RemoteServer, command: &str) -> Result<AgentResponse> {
    exchange(
        &remote.endpoint,
        &format!("AUTH {} {}", remote.token, command),
    )
}

pub fn load(name: &str) -> Result<RemoteServer> {
    validate_name(name)?;
    let path = server_path(name)?;
    let bytes = fs::read(&path)
        .with_context(|| format!("server '{name}' is not paired ({})", path.display()))?;
    Ok(serde_json::from_slice(&bytes)?)
}

fn save(remote: &RemoteServer) -> Result<()> {
    let path = server_path(&remote.name)?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let data = serde_json::to_vec_pretty(remote)?;
    fs::write(&path, data)?;
    set_private_permissions(&path)?;
    Ok(())
}

fn exchange(endpoint: &str, request: &str) -> Result<AgentResponse> {
    let mut stream = TcpStream::connect(endpoint)
        .with_context(|| format!("failed to connect to Anvil Agent at {endpoint}"))?;
    stream.set_read_timeout(Some(Duration::from_secs(10)))?;
    stream.set_write_timeout(Some(Duration::from_secs(10)))?;

    stream.write_all(request.as_bytes())?;
    stream.write_all(b"\n")?;
    stream.flush()?;

    let mut line = String::new();
    BufReader::new(stream).read_line(&mut line)?;
    if line.trim().is_empty() {
        bail!("agent closed the connection without a response");
    }

    serde_json::from_str(line.trim()).context("invalid response from Anvil Agent")
}

fn server_path(name: &str) -> Result<PathBuf> {
    Ok(config_root()?.join("servers").join(format!("{name}.json")))
}

fn config_root() -> Result<PathBuf> {
    if let Some(value) = env::var_os("XDG_CONFIG_HOME") {
        return Ok(PathBuf::from(value).join("anvil"));
    }
    let home = env::var_os("HOME").ok_or_else(|| anyhow!("HOME is not set"))?;
    Ok(PathBuf::from(home).join(".config/anvil"))
}

fn validate_name(name: &str) -> Result<()> {
    if name.is_empty()
        || !name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'))
    {
        bail!("server name may only contain letters, numbers, '-', '_' and '.'");
    }
    Ok(())
}

#[cfg(unix)]
fn set_private_permissions(path: &std::path::Path) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(path, fs::Permissions::from_mode(0o600))?;
    Ok(())
}

#[cfg(not(unix))]
fn set_private_permissions(_path: &std::path::Path) -> Result<()> {
    Ok(())
}
