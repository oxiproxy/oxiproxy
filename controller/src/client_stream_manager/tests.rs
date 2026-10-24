use super::*;
use common::protocol::auth::ClientAuthProvider;
use sea_orm::{ConnectionTrait, Database};
use sea_orm_migration::MigratorTrait;

async fn database() -> DatabaseConnection {
    let db = Database::connect("sqlite::memory:").await.unwrap();
    crate::migration::Migrator::up(&db, None).await.unwrap();
    db.execute_unprepared(
        "INSERT INTO client (id,name,token,is_online,total_bytes_sent,total_bytes_received,created_at,updated_at)
         VALUES (1,'client','test',0,0,0,CURRENT_TIMESTAMP,CURRENT_TIMESTAMP)",
    )
    .await
    .unwrap();
    db
}

async fn client(db: &DatabaseConnection) -> client::Model {
    Client::find_by_id(1).one(db).await.unwrap().unwrap()
}

#[tokio::test]
async fn old_session_cleanup_preserves_the_reconnected_client_and_commands() {
    let db = database().await;
    let manager = ClientStreamManager::new();
    let (old_tx, _old_rx) = mpsc::channel(4);
    manager.register(1, old_tx.clone(), &db).await.unwrap();
    let (new_tx, mut new_rx) = mpsc::channel(4);
    manager.register(1, new_tx.clone(), &db).await.unwrap();

    manager.unregister(1, &old_tx, &db).await.unwrap();
    assert!(client(&db).await.is_online);
    assert!(client(&db).await.last_online_at.is_some());

    let command_manager = manager.clone();
    let command = tokio::spawn(async move { command_manager.send_restart(1).await });
    let message = tokio::time::timeout(Duration::from_secs(2), new_rx.recv())
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    let Some(oxiproxy::controller_to_client_message::Payload::Restart(request)) = message.payload
    else {
        panic!("restart must reach the new session");
    };
    manager
        .complete_pending_request(
            1,
            &oxiproxy::AgentClientResponse {
                request_id: request.request_id,
                result: Some(oxiproxy::agent_client_response::Result::SoftwareUpdate(
                    oxiproxy::SoftwareUpdateResponse {
                        success: true,
                        ..Default::default()
                    },
                )),
            },
        )
        .await;
    command.await.unwrap().unwrap();

    manager.unregister(1, &new_tx, &db).await.unwrap();
    assert!(!client(&db).await.is_online);
    assert!(manager.streams.read().await.is_empty());
}

#[tokio::test]
async fn tunnel_reports_cannot_override_control_session_status() {
    let db = database().await;
    let manager = ClientStreamManager::new();
    let auth = crate::local_auth_provider::LocalControllerAuthProvider::new();
    let (tx, _rx) = mpsc::channel(4);
    manager.register(1, tx.clone(), &db).await.unwrap();

    // A second node disconnects while the client's control session is healthy.
    auth.set_client_online(1, false).await.unwrap();
    assert!(client(&db).await.is_online);
    manager.sync_online_status(&db).await.unwrap();
    assert!(client(&db).await.is_online);

    manager.unregister(1, &tx, &db).await.unwrap();
    auth.set_client_online(1, true).await.unwrap();
    assert!(!client(&db).await.is_online);
}

#[tokio::test]
async fn live_status_overrides_stale_database_values_in_both_directions() {
    let db = database().await;
    let manager = ClientStreamManager::new();
    let mut clients = vec![client(&db).await];
    let (tx, _rx) = mpsc::channel(4);
    manager.register(1, tx, &db).await.unwrap();
    manager.apply_online_status(&mut clients).await;
    assert!(clients[0].is_online);

    // After a Controller restart, persisted online=true is not a live session.
    let restarted_manager = ClientStreamManager::new();
    restarted_manager.apply_online_status(&mut clients).await;
    assert!(!clients[0].is_online);
    restarted_manager.sync_online_status(&db).await.unwrap();
    assert!(!client(&db).await.is_online);
}

#[tokio::test]
async fn closed_response_stream_is_offline_before_cleanup_runs() {
    let db = database().await;
    let manager = ClientStreamManager::new();
    let (tx, rx) = mpsc::channel(4);
    manager.register(1, tx.clone(), &db).await.unwrap();
    drop(rx);

    let mut clients = vec![client(&db).await];
    manager.apply_online_status(&mut clients).await;
    assert!(!clients[0].is_online);
    manager.sync_online_status(&db).await.unwrap();
    assert!(!client(&db).await.is_online);
    manager.unregister(1, &tx, &db).await.unwrap();
    manager.unregister(1, &tx, &db).await.unwrap();
}

#[tokio::test]
async fn reconnect_and_health_sync_leave_the_current_session_online() {
    let db = database().await;
    let manager = ClientStreamManager::new();
    let (mut tx, mut rx) = mpsc::channel(4);
    manager.register(1, tx.clone(), &db).await.unwrap();

    for _ in 0..20 {
        let (next_tx, next_rx) = mpsc::channel(4);
        let (registered, synced, removed) = tokio::join!(
            manager.register(1, next_tx.clone(), &db),
            manager.sync_online_status(&db),
            manager.unregister(1, &tx, &db),
        );
        registered.unwrap();
        synced.unwrap();
        removed.unwrap();
        assert!(client(&db).await.is_online);
        tx = next_tx;
        rx = next_rx;
    }
    assert!(!rx.is_closed());
    manager.unregister(1, &tx, &db).await.unwrap();
    assert!(!client(&db).await.is_online);
}
