use super::*;

#[path = "../../../common/tests/support/controller.rs"]
mod controller;

async fn register(controller: &mut controller::Controller) -> controller::NodeSession {
    let (mut inbound, outbound) = controller.nodes.recv().await.unwrap();
    assert!(matches!(
        inbound.message().await.unwrap().unwrap().payload,
        Some(AgentPayload::Register(_))
    ));
    outbound
        .send(Ok(oxiproxy::ControllerToAgentMessage {
            payload: Some(ControllerPayload::RegisterResponse(
                oxiproxy::NodeRegisterResponse {
                    node_id: 1,
                    node_name: "test".into(),
                    tunnel_protocol: "tcp".into(),
                    tunnel_port: 17000,
                    ..Default::default()
                },
            )),
        }))
        .await
        .unwrap();
    (inbound, outbound)
}

#[tokio::test]
async fn node_reconnects_after_silent_loss_and_stops_old_heartbeats() {
    let mut controller = controller::Controller::start().await;
    let url = controller.url.clone();
    let connect = tokio::spawn(async move {
        AgentGrpcClient::connect_and_authenticate(&url, "test-token", "tcp", None).await
    });
    let _first_session = register(&mut controller).await;
    let (client, mut commands, ..) = connect.await.unwrap().unwrap();
    tokio::time::pause();
    tokio::task::yield_now().await;
    tokio::time::advance(connection::HEARTBEAT_TIMEOUT).await;
    client.wait_for_disconnect().await;
    assert!(commands.recv().await.is_none());
    tokio::time::resume();

    let reconnect_client = client.clone();
    let url = controller.url.clone();
    let reconnect = tokio::spawn(async move {
        reconnect_client
            .reconnect(&url, "test-token", "tcp", None)
            .await
    });
    let (mut inbound, outbound) = register(&mut controller).await;
    let (_commands, ..) = reconnect.await.unwrap().unwrap();

    // A healthy new stream has one heartbeat every 15 seconds. An old task
    // using the shared sender would produce additional heartbeats here.
    tokio::time::pause();
    tokio::task::yield_now().await;
    tokio::time::advance(connection::HEARTBEAT_INTERVAL).await;
    tokio::time::resume();
    let message = tokio::time::timeout(Duration::from_secs(2), inbound.message())
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    assert!(matches!(message.payload, Some(AgentPayload::Heartbeat(_))));
    assert!(
        tokio::time::timeout(Duration::from_millis(100), inbound.message())
            .await
            .is_err()
    );

    outbound
        .send(Err(tonic::Status::unavailable("disconnected")))
        .await
        .unwrap();
    tokio::time::timeout(Duration::from_secs(2), client.wait_for_disconnect())
        .await
        .unwrap();
}

#[tokio::test]
async fn replacing_a_sender_is_not_blocked_by_a_full_old_queue() {
    let (old_tx, _old_rx) = mpsc::channel(1);
    let message = || oxiproxy::AgentServerMessage { payload: None };
    old_tx.send(message()).await.unwrap();
    let sender = SharedGrpcSender::new(old_tx);
    let blocked_sender = sender.clone();
    let blocked = tokio::spawn(async move { blocked_sender.send(message()).await });
    tokio::task::yield_now().await;

    let (new_tx, mut new_rx) = mpsc::channel(1);
    tokio::time::timeout(Duration::from_secs(1), sender.replace(new_tx))
        .await
        .unwrap();
    sender.send(message()).await.unwrap();
    assert!(new_rx.recv().await.is_some());
    tokio::time::pause();
    tokio::time::advance(SEND_TIMEOUT).await;
    assert!(blocked.await.unwrap().is_err());
}

#[tokio::test]
async fn a_controller_that_never_registers_cannot_stall_retries_forever() {
    let mut controller = controller::Controller::start().await;
    let url = controller.url.clone();
    let connect = tokio::spawn(async move {
        AgentGrpcClient::connect_and_authenticate(&url, "test-token", "tcp", None).await
    });
    let (mut inbound, _outbound) = controller.nodes.recv().await.unwrap();
    inbound.message().await.unwrap().unwrap();
    tokio::time::pause();
    let result = tokio::time::timeout(AUTH_TIMEOUT * 2, connect)
        .await
        .unwrap()
        .unwrap();
    assert!(result.err().unwrap().to_string().contains("超时"));
}
