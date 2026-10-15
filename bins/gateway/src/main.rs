mod app;
mod config;

use anyhow::Result;
use sies_infra::{init_tracing, load_config};
use tracing::error;

use crate::{app::App, config::GatewayConfig};

#[tokio::main]
async fn main() -> Result<()> {
    init_tracing();

    let config_path =
        std::env::var("SIES_CONFIG").unwrap_or_else(|_| "conf/gateway.toml".to_string());

    let config: GatewayConfig = match load_config(&config_path) {
        Ok(c) => c,
        Err(e) => {
            if config_path == "conf/gateway.toml" {
                tracing::warn!("Config file not found, using default configuration");
                GatewayConfig {
                    host: "0.0.0.0".to_string(),
                    port: 8082,
                    node_endpoint: "http://127.0.0.1:8080".to_string(),
                    verifier_endpoint: "http://127.0.0.1:8081".to_string(),
                    api_prefix: "/api/v1".to_string(),
                }
            } else {
                error!(error = %e, "Failed to load gateway config");
                return Err(e.into());
            }
        }
    };

    let app = App::new(config);
    if let Err(e) = app.run().await {
        error!(error = %e, "sies-gateway exited with error");
        return Err(e);
    }

    Ok(())
}
