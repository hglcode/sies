use crate::config::VerifierConfig;
use tracing::info;

pub struct App {
    config: VerifierConfig,
}

impl App {
    pub fn new(config: VerifierConfig) -> Self {
        Self { config }
    }

    pub async fn run(self) -> anyhow::Result<()> {
        info!(
            host = %self.config.host,
            port = self.config.port,
            data_dir = %self.config.data_dir,
            node_endpoint = %self.config.node_endpoint,
            "Starting sies-verifier"
        );

        // TODO: 连接 sies-node RPC
        // TODO: 初始化 ZK Prover / Verifier 引擎
        // TODO: 启动证明生成与提交循环

        info!("sies-verifier is running. Press Ctrl+C to stop.");

        tokio::signal::ctrl_c().await?;

        info!("Shutdown signal received, stopping sies-verifier...");
        // TODO: 优雅关闭（断开 RPC、刷盘等）
        info!("sies-verifier stopped gracefully.");
        Ok(())
    }
}
