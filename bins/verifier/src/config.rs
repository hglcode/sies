use serde::Deserialize;

#[derive(Debug, Deserialize)]
pub struct VerifierConfig {
    /// 验证器监听地址
    #[serde(default = "default_host")]
    pub host: String,

    /// 验证器端口
    #[serde(default = "default_port")]
    pub port: u16,

    /// 状态数据存储目录
    #[serde(default = "default_data_dir")]
    pub data_dir: String,

    /// 连接的 sies-node RPC 端点
    #[serde(default = "default_node_endpoint")]
    pub node_endpoint: String,
}

fn default_host() -> String {
    "127.0.0.1".to_string()
}
fn default_port() -> u16 {
    8081
}
fn default_data_dir() -> String {
    "./data/verifier".to_string()
}
fn default_node_endpoint() -> String {
    "http://127.0.0.1:8080".to_string()
}
