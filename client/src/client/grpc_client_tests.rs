use super::*;

#[path = "../../../common/tests/support/controller.rs"]
mod controller;

async fn connect(
    controller: &mut controller::Controller,
) -> (
    mpsc::Receiver<Vec<ClientServerProxyGroup>>,
    controller::ClientSession,
) {
    let url = controller.url.clone();
    let connect = tokio::spawn(async move {
        connect_and_run(&url, "test-token", None, LogCollector::new(10)).await
    });
    let (mut inbound, outbound) = controller.clients.recv().await.unwrap();
    assert!(matches!(
        inbound.message().await.unwrap().unwrap().payload,
        Some(ClientPayload::Auth(_))
    ));
    outbound
        .send(Ok(oxiproxy::ControllerToClientMessage {
            payload: Some(ControllerPayload::AuthResponse(
                oxiproxy::ClientAuthResponse {
                    success: true,
                    client_id: 1,
                    client_name: "test".into(),
                    ..Default::default()
                },
            )),
        }))
        .await
        .unwrap();
    let (_, _, updates) = connect.await.unwrap().unwrap();
    (updates, (inbound, outbound))
}

#[tokio::test]
async fn stream_error_releases_updates_and_allows_a_new_connection() {
    let mut controller = controller::Controller::start().await;
    let (mut updates, (_inbound, outbound)) = connect(&mut controller).await;
    outbound
        .send(Err(tonic::Status::unavailable("disconnected")))
        .await
        .unwrap();
    assert!(tokio::time::timeout(Duration::from_secs(2), updates.recv())
        .await
        .unwrap()
        .is_none());

    let (mut updates, (_inbound, outbound)) = connect(&mut controller).await;
    outbound
        .send(Ok(oxiproxy::ControllerToClientMessage {
            payload: Some(ControllerPayload::ProxyUpdate(
                oxiproxy::ProxyListUpdate::default(),
            )),
        }))
        .await
        .unwrap();
    assert!(tokio::time::timeout(Duration::from_secs(2), updates.recv())
        .await
        .unwrap()
        .unwrap()
        .is_empty());
}

#[tokio::test]
async fn silent_controller_closes_updates_to_trigger_reconnection() {
    let mut controller = controller::Controller::start().await;
    let (mut updates, _session) = connect(&mut controller).await;
    tokio::time::pause();
    tokio::task::yield_now().await;
    tokio::time::advance(connection::HEARTBEAT_TIMEOUT).await;
    assert!(updates.recv().await.is_none());
}

#[tokio::test]
async fn a_controller_that_never_authenticates_cannot_stall_retries_forever() {
    let mut controller = controller::Controller::start().await;
    let url = controller.url.clone();
    let connect = tokio::spawn(async move {
        connect_and_run(&url, "test-token", None, LogCollector::new(10)).await
    });
    let (mut inbound, _outbound) = controller.clients.recv().await.unwrap();
    inbound.message().await.unwrap().unwrap();
    tokio::time::pause();
    let result = tokio::time::timeout(AUTH_TIMEOUT * 2, connect)
        .await
        .unwrap()
        .unwrap();
    assert!(result.err().unwrap().to_string().contains("超时"));
}
