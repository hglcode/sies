use serde::Deserialize;

#[derive(Debug, Deserialize)]
pub struct GatewayConfig {
    /// 网关监听地址
    #[serde(default = "default_host")]
    pub host: String,

    /// 网关端口
    #[serde(default = "default_port")]
    pub port: u16,

    /// 上游 sies-node RPC 端点
    #[serde(default = "default_node_endpoint")]
    pub node_endpoint: String,

    /// 上游 sies-verifier RPC 端点
    #[serde(default = "default_verifier_endpoint")]
    pub verifier_endpoint: String,

    /// API 路由前缀
    #[serde(default = "default_api_prefix")]
    pub api_prefix: String,
}

fn default_host() -> String {
    "0.0.0.0".to_string()
}
fn default_port() -> u16 {
    8082
}
fn default_node_endpoint() -> String {
    "http://127.0.0.1:8080".to_string()
}
fn default_verifier_endpoint() -> String {
    "http://127.0.0.1:8081".to_string()
}
fn default_api_prefix() -> String {
    "/api/v1".to_string()
}
