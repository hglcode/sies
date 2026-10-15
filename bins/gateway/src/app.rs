use crate::config::GatewayConfig;
use tracing::info;

pub struct App {
    config: GatewayConfig,
}

impl App {
    pub fn new(config: GatewayConfig) -> Self {
        Self { config }
    }

    pub async fn run(self) -> anyhow::Result<()> {
        info!(
            host = %self.config.host,
            port = self.config.port,
            api_prefix = %self.config.api_prefix,
            node_endpoint = %self.config.node_endpoint,
            verifier_endpoint = %self.config.verifier_endpoint,
            "Starting sies-gateway"
        );

        // TODO: 初始化 HTTP/gRPC 服务器
        // TODO: 注册路由与中间件（认证、限流、日志）
        // TODO: 建立到 node / verifier 的上游连接池

        info!("sies-gateway is running. Press Ctrl+C to stop.");

        tokio::signal::ctrl_c().await?;

        info!("Shutdown signal received, stopping sies-gateway...");
        // TODO: 优雅关闭（drain 活跃请求、断开上游连接）
        info!("sies-gateway stopped gracefully.");
        Ok(())
    }
}
