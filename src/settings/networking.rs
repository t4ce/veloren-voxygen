use hashbrown::HashSet;
use serde::{Deserialize, Serialize};

const DEFAULT_AUTH_SERVER: &str = "https://auth.veloren.net";

/// `NetworkingSettings` stores server and networking settings.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct NetworkingSettings {
    pub username: String,
    pub servers: Vec<String>,
    pub default_server: String,
    pub trusted_auth_servers: HashSet<String>,
    pub use_srv: bool,
    pub use_quic: bool,
    pub validate_tls: bool,
    pub player_physics_behavior: bool,
    pub lossy_terrain_compression: bool,
}

impl NetworkingSettings {
    /// The official HTTPS provider is built into this client's trust policy,
    /// including when an older settings file has an empty trusted-server list.
    pub fn is_auth_server_trusted(&self, server: &str) -> bool {
        server == DEFAULT_AUTH_SERVER || self.trusted_auth_servers.contains(server)
    }
}

impl Default for NetworkingSettings {
    fn default() -> Self {
        Self {
            username: "".to_string(),
            servers: vec!["server.veloren.net".to_string()],
            default_server: "server.veloren.net".to_string(),
            trusted_auth_servers: [DEFAULT_AUTH_SERVER]
                .iter()
                .map(|s| s.to_string())
                .collect(),
            use_srv: true,
            use_quic: false,
            validate_tls: true,
            player_physics_behavior: false,
            lossy_terrain_compression: false,
        }
    }
}
