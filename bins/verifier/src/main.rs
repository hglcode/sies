mod app;
mod config;

use anyhow::Result;
use sies_infra::{init_tracing, load_config};
use tracing::error;

use crate::{app::App, config::VerifierConfig};

#[tokio::main]
async fn main() -> Result<()> {
    init_tracing();

    let config_path =
        std::env::var("SIES_CONFIG").unwrap_or_else(|_| "conf/verifier.toml".to_string());

    let config: VerifierConfig = match load_config(&config_path) {
        Ok(c) => c,
        Err(e) => {
            if config_path == "conf/verifier.toml" {
                tracing::warn!("Config file not found, using default configuration");
                VerifierConfig {
                    host: "127.0.0.1".to_string(),
                    port: 8081,
                    data_dir: "./data/verifier".to_string(),
                    node_endpoint: "http://127.0.0.1:8080".to_string(),
                }
            } else {
                error!(error = %e, "Failed to load verifier config");
                return Err(e.into());
            }
        }
    };

    let app = App::new(config);
    if let Err(e) = app.run().await {
        error!(error = %e, "sies-verifier exited with error");
        return Err(e);
    }

    Ok(())
}
