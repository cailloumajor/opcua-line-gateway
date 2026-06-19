use opcua::client::{ClientBuilder, IdentityToken};
use opcua::types::{NodeId, TimestampsToReturn};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    env_logger::init();

    let mut client = ClientBuilder::new()
        .application_name("Test")
        .application_uri("urn:Test")
        .session_retry_limit(3)
        .client()
        .unwrap();
    let (session, event_loop) = client
        .connect_to_matching_endpoint("opc.tcp://192.168.42.1:4840", IdentityToken::Anonymous)
        .await
        .unwrap();
    let handle = event_loop.spawn();
    session.wait_for_connection().await;

    let node_id = NodeId::new(3, "\"dbBaLTraca\".\"General\"");
    let data_value = session
        .read(&[node_id.into()], TimestampsToReturn::Neither, 0.0)
        .await
        .unwrap();
    dbg!(&data_value[0]);

    session.disconnect().await.unwrap();
    handle.await.unwrap();

    Ok(())
}
