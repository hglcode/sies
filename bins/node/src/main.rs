mod app;
mod config;

use anyhow::Result;
use sies_infra::{init_tracing, load_config};
use tracing::error;

use crate::{app::App, config::NodeConfig};

#[tokio::main]
async fn main() -> Result<()> {
    // 1. 初始化 Tracing
    init_tracing();

    // 2. 加载配置 (默认读取 config/node.toml)
    let config_path = std::env::var("SIES_CONFIG").unwrap_or_else(|_| "conf/node.toml".to_string());
    let config: NodeConfig = match load_config(&config_path) {
        Ok(c) => c,
        Err(e) => {
            // 如果配置文件不存在且是默认路径，则使用默认配置
            if config_path == "config/node.toml" {
                tracing::warn!("Config file not found, using default configuration");
                NodeConfig {
                    host: "127.0.0.1".to_string(),
                    port: 8080,
                    data_dir: "./data/node".to_string(),
                }
            } else {
                error!(error = %e, "Failed to load config");
                return Err(e.into());
            }
        }
    };

    // 3. 启动应用
    let app = App::new(config);
    if let Err(e) = app.run().await {
        error!(error = %e, "Application exited with error");
        return Err(e);
    }

    Ok(())
}
