use serde::Deserialize;

#[derive(Debug, Deserialize)]
pub struct NodeConfig {
    #[serde(default = "default_host")]
    pub host: String,

    #[serde(default = "default_port")]
    pub port: u16,

    #[serde(default = "default_data_dir")]
    pub data_dir: String,
}

fn default_host() -> String {
    "127.0.0.1".to_string()
}

fn default_port() -> u16 {
    8080
}

fn default_data_dir() -> String {
    "./data/node".to_string()
}
