use crate::config::NodeConfig;
use tracing::info;

pub struct App {
    config: NodeConfig,
}

impl App {
    pub fn new(config: NodeConfig) -> Self {
        Self { config }
    }

    pub async fn run(self) -> anyhow::Result<()> {
        info!(
            host = %self.config.host,
            port = self.config.port,
            data_dir = %self.config.data_dir,
            "Starting sies-node"
        );

        // TODO: 初始化 P2P 网络
        // TODO: 初始化 RPC 服务
        // TODO: 初始化 Executor 与 Consensus 模块

        info!("sies-node is running. Press Ctrl+C to stop.");

        // 保持运行，直到收到退出信号
        tokio::signal::ctrl_c().await?;

        info!("Shutdown signal received, stopping sies-node...");

        // TODO: 执行优雅关闭逻辑 (如刷盘、断开 P2P 连接)

        info!("sies-node stopped gracefully.");
        Ok(())
    }
}
