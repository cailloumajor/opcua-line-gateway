use std::env;
use std::path::PathBuf;

use anyhow::{Context as _, anyhow};
use opcua_line_gateway_config::LineGatewayConfig;
use tracing_subscriber::EnvFilter;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;

use crate::opcua::create_client;

mod opcua;

fn main() -> anyhow::Result<()> {
    // Initialize tracing.
    tracing_subscriber::registry()
        .with(tracing_subscriber::fmt::layer())
        // Configure log levels using RUST_LOG environment variable.
        .with(EnvFilter::from_default_env())
        .init();

    let Some(config_path) = env::args_os().nth(1).map(PathBuf::from) else {
        return Err(anyhow!(
            "Failed to get configuration file path from first positional argument"
        ));
    };
    let config = LineGatewayConfig::from_toml_file(config_path)
        .context("Failed to get configuration from file")?;

    let _client = create_client(&config).context("Failed to create OPC-UA client")?;

    Ok(())
}
