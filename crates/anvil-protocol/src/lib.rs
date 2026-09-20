use serde::{Deserialize, Serialize};

pub const PROTOCOL_VERSION: u16 = 1;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Envelope<T> {
    pub request_id: u64,
    pub protocol_version: u16,
    pub body: T,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Request {
    Ping,
    GetServerInfo,
    GetPlayers,
    ListDir { path: String },
    ReadFile { path: String },
    HashFile { path: String },
    WriteFile { path: String, size: u64, sha256: String },
    ExecConsole { command: String },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Response {
    Pong,
    Ack,
    Error { code: String, message: String },
    ServerInfo(ServerInfo),
    Players(Vec<PlayerInfo>),
    Directory(Vec<DirEntry>),
    File { path: String, size: u64, sha256: String },
    Hash { path: String, sha256: String },
    ConsoleResult { accepted: bool },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServerInfo {
    pub name: String,
    pub minecraft_version: String,
    pub implementation: String,
    pub online_players: u32,
    pub max_players: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlayerInfo {
    pub name: String,
    pub uuid: String,
    pub ping_ms: i32,
    pub world: String,
    pub game_mode: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DirEntry {
    pub name: String,
    pub path: String,
    pub kind: EntryKind,
    pub size: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EntryKind {
    File,
    Directory,
    Symlink,
}
