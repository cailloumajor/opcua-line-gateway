use anyhow::Context;
use opcua_line_gateway_config::LineGatewayConfig;
use schemars::schema_for;

fn main() -> anyhow::Result<()> {
    let schema = schema_for!(LineGatewayConfig);
    let json = serde_json::to_string_pretty(&schema).context("Failed to serialize JSON schema")?;

    println!("{json}");

    Ok(())
}
