use anyhow::Context;
use opcua_line_gateway_config::LineGatewayConfig;
use schemars::schema_for;

fn main() -> anyhow::Result<()> {
    let mut schema = schema_for!(LineGatewayConfig);
    schema.insert("x-tombi-string-formats".to_string(), ["uri"].into());
    let json = serde_json::to_string_pretty(&schema).context("Failed to serialize JSON schema")?;

    println!("{json}");

    Ok(())
}
