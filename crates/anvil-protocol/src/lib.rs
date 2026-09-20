use serde::{Deserialize, Serialize};

pub const PROTOCOL_VERSION: u16 = 1;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServerInfo {
    pub server_id: String,
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

#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum AgentResponse {
    Paired {
        server_id: String,
        token: String,
    },
    ServerInfo {
        server_id: String,
        name: String,
        minecraft_version: String,
        implementation: String,
        online_players: u32,
        max_players: u32,
    },
    Players {
        players: Vec<PlayerInfo>,
    },
    Error {
        code: String,
        message: String,
    },
}

impl AgentResponse {
    pub fn into_server_info(self) -> Option<ServerInfo> {
        match self {
            Self::ServerInfo {
                server_id,
                name,
                minecraft_version,
                implementation,
                online_players,
                max_players,
            } => Some(ServerInfo {
                server_id,
                name,
                minecraft_version,
                implementation,
                online_players,
                max_players,
            }),
            _ => None,
        }
    }
}
