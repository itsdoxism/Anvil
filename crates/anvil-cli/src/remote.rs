use anyhow::{anyhow, bail, Context, Result};
use anvil_protocol::{AgentResponse, DirEntry};
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    env,
    fs,
    io::{BufRead, BufReader, Read, Write},
    net::TcpStream,
    path::{Path, PathBuf},
    time::Duration,
};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RemoteServer {
    pub name: String,
    pub endpoint: String,
    pub server_id: String,
    pub token: String,
}

#[derive(Debug, Clone)]
pub struct RemoteFile {
    pub path: String,
    pub bytes: Vec<u8>,
    pub sha256: String,
}

pub fn pair(name: &str, endpoint: &str, code: &str) -> Result<RemoteServer> {
    validate_name(name)?;
    let response = exchange_line(endpoint, &format!("PAIR {}", code.trim()))?;
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
    exchange_line(
        &remote.endpoint,
        &format!("AUTH {} {}", remote.token, command),
    )
}

pub fn list_dir(remote: &RemoteServer, path: &str) -> Result<Vec<DirEntry>> {
    let encoded = encode_path(path);
    match request(remote, &format!("LIST {encoded}"))? {
        AgentResponse::Directory { entries, .. } => Ok(entries),
        AgentResponse::Error { code, message } => bail!("{code}: {message}"),
        other => bail!("unexpected agent response: {other:?}"),
    }
}

pub fn read_file(remote: &RemoteServer, path: &str) -> Result<RemoteFile> {
    match try_read_file(remote, path)? {
        Some(file) => Ok(file),
        None => bail!("remote file not found: {path}"),
    }
}

pub fn try_read_file(remote: &RemoteServer, path: &str) -> Result<Option<RemoteFile>> {
    let mut stream = connect(&remote.endpoint)?;
    let encoded = encode_path(path);
    write_request(
        &mut stream,
        &format!("AUTH {} READ {encoded}", remote.token),
    )?;

    let mut reader = BufReader::new(stream);
    let response = read_response(&mut reader)?;
    match response {
        AgentResponse::File {
            path,
            size,
            sha256,
        } => {
            let size: usize = size
                .try_into()
                .context("remote file is too large for this platform")?;
            let mut bytes = vec![0_u8; size];
            reader.read_exact(&mut bytes)?;
            let actual = sha256_bytes(&bytes);
            if actual != sha256 {
                bail!("remote checksum mismatch for {path}");
            }
            Ok(Some(RemoteFile {
                path,
                bytes,
                sha256,
            }))
        }
        AgentResponse::Error { code, .. } if code == "not_found" => Ok(None),
        AgentResponse::Error { code, message } => bail!("{code}: {message}"),
        other => bail!("unexpected agent response: {other:?}"),
    }
}

pub fn write_file(remote: &RemoteServer, path: &str, bytes: &[u8]) -> Result<String> {
    let mut stream = connect(&remote.endpoint)?;
    let encoded = encode_path(path);
    let sha256 = sha256_bytes(bytes);
    write_request(
        &mut stream,
        &format!(
            "AUTH {} WRITE {encoded} {} {sha256}",
            remote.token,
            bytes.len()
        ),
    )?;
    stream.write_all(bytes)?;
    stream.flush()?;

    let mut reader = BufReader::new(stream);
    match read_response(&mut reader)? {
        AgentResponse::Written {
            path: returned_path,
            size,
            sha256: returned_hash,
        } => {
            if returned_hash != sha256 || size != bytes.len() as u64 {
                bail!("agent returned inconsistent write metadata for {returned_path}");
            }
            Ok(returned_hash)
        }
        AgentResponse::Error { code, message } => bail!("{code}: {message}"),
        other => bail!("unexpected agent response: {other:?}"),
    }
}

pub fn store_backup(server: &RemoteServer, file: &RemoteFile) -> Result<PathBuf> {
    let root = data_root()?.join("objects");
    let object = root
        .join(&file.sha256[..2])
        .join(&file.sha256[2..]);
    if !object.exists() {
        if let Some(parent) = object.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(&object, &file.bytes)?;
    }

    let record_dir = data_root()?.join("backups").join(&server.name);
    fs::create_dir_all(&record_dir)?;
    let safe_name = file.path.replace('/', "__");
    let record = record_dir.join(format!("{}--{}.json", &file.sha256[..12], safe_name));
    let metadata = serde_json::json!({
        "server": server.name,
        "server_id": server.server_id,
        "remote_path": file.path,
        "sha256": file.sha256,
        "size": file.bytes.len(),
        "object": object,
    });
    fs::write(&record, serde_json::to_vec_pretty(&metadata)?)?;
    Ok(object)
}

pub fn load(name: &str) -> Result<RemoteServer> {
    validate_name(name)?;
    let path = server_path(name)?;
    let bytes = fs::read(&path)
        .with_context(|| format!("server '{name}' is not paired ({})", path.display()))?;
    Ok(serde_json::from_slice(&bytes)?)
}

pub fn sha256_bytes(bytes: &[u8]) -> String {
    hex::encode(Sha256::digest(bytes))
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

fn exchange_line(endpoint: &str, request: &str) -> Result<AgentResponse> {
    let mut stream = connect(endpoint)?;
    write_request(&mut stream, request)?;
    let mut reader = BufReader::new(stream);
    read_response(&mut reader)
}

fn connect(endpoint: &str) -> Result<TcpStream> {
    let stream = TcpStream::connect(endpoint)
        .with_context(|| format!("failed to connect to Anvil Agent at {endpoint}"))?;
    stream.set_read_timeout(Some(Duration::from_secs(30)))?;
    stream.set_write_timeout(Some(Duration::from_secs(30)))?;
    Ok(stream)
}

fn write_request(stream: &mut TcpStream, request: &str) -> Result<()> {
    stream.write_all(request.as_bytes())?;
    stream.write_all(b"\n")?;
    stream.flush()?;
    Ok(())
}

fn read_response(reader: &mut impl BufRead) -> Result<AgentResponse> {
    let mut line = String::new();
    reader.read_line(&mut line)?;
    if line.trim().is_empty() {
        bail!("agent closed the connection without a response");
    }
    serde_json::from_str(line.trim()).context("invalid response from Anvil Agent")
}

fn encode_path(path: &str) -> String {
    URL_SAFE_NO_PAD.encode(path.as_bytes())
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

fn data_root() -> Result<PathBuf> {
    if let Some(value) = env::var_os("XDG_DATA_HOME") {
        return Ok(PathBuf::from(value).join("anvil"));
    }
    let home = env::var_os("HOME").ok_or_else(|| anyhow!("HOME is not set"))?;
    Ok(PathBuf::from(home).join(".local/share/anvil"))
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
fn set_private_permissions(path: &Path) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(path, fs::Permissions::from_mode(0o600))?;
    Ok(())
}

#[cfg(not(unix))]
fn set_private_permissions(_path: &Path) -> Result<()> {
    Ok(())
}
