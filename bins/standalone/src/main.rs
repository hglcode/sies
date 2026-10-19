use anyhow::Result;
use clap::Parser;
use sies_standalone::{StandaloneConfig, StandaloneRuntime};
use std::path::PathBuf;

#[derive(Parser, Debug)]
#[command(author, version, about = "SIES Standalone Node")]
struct Cli {
    /// 数据存储目录
    #[arg(short, long, default_value = "./data/standalone")]
    data_dir: PathBuf,
}

#[tokio::main]
async fn main() -> Result<()> {
    // 复用 infra 层的 tracing 初始化
    sies_infra::tracing::init_tracing();

    let cli = Cli::parse();
    let config = StandaloneConfig {
        data_dir: cli.data_dir,
    };

    tracing::info!("Starting SIES Standalone...");
    let runtime = StandaloneRuntime::start(&config)?;
    tracing::info!(
        root = hex::encode(runtime.current_root()),
        "Standalone ready"
    );

    // 阻塞等待退出信号
    tokio::signal::ctrl_c().await?;
    tracing::info!("Shutting down standalone node");

    Ok(())
}
