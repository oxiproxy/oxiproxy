use super::*;
use sea_orm::{ConnectionTrait, Database};
use sea_orm_migration::MigratorTrait;
use std::sync::Arc;

async fn database() -> DatabaseConnection {
    let db = Database::connect("sqlite::memory:").await.unwrap();
    crate::migration::Migrator::up(&db, None).await.unwrap();
    db.execute_unprepared(
        "INSERT INTO node (id,name,url,secret,created_at,updated_at)
         VALUES (1,'first','http://first','first-token',CURRENT_TIMESTAMP,CURRENT_TIMESTAMP),
                (2,'second','http://second','second-token',CURRENT_TIMESTAMP,CURRENT_TIMESTAMP)",
    )
    .await
    .unwrap();
    db
}

async fn node(db: &DatabaseConnection, id: i64) -> node::Model {
    Node::find_by_id(id).one(db).await.unwrap().unwrap()
}

#[tokio::test]
async fn old_session_cleanup_preserves_reconnected_node_and_log_commands() {
    let db = database().await;
    let manager = Arc::new(NodeManager::new());
    let (old_tx, _old_rx) = mpsc::channel(4);
    manager
        .register_node_stream(1, old_tx.clone(), &db)
        .await
        .unwrap();
    let (new_tx, mut new_rx) = mpsc::channel(4);
    manager
        .register_node_stream(1, new_tx.clone(), &db)
        .await
        .unwrap();

    manager
        .unregister_node_stream(1, &old_tx, &db)
        .await
        .unwrap();
    assert!(node(&db, 1).await.is_online);
    assert_eq!(manager.get_loaded_node_ids().await, vec![1]);

    let command_manager = manager.clone();
    let command = tokio::spawn(async move { command_manager.get_node_logs(1, 100).await });
    let message = tokio::time::timeout(Duration::from_secs(2), new_rx.recv())
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    let Some(ControllerPayload::GetNodeLogs(request)) = message.payload else {
        panic!("log request must reach the new session");
    };
    manager
        .complete_pending_request(
            1,
            &oxiproxy::AgentServerResponse {
                request_id: request.request_id,
                result: Some(AgentResult::NodeLogs(oxiproxy::NodeLogsResponse {
                    logs: vec![oxiproxy::LogEntry {
                        timestamp: "now".into(),
                        level: "INFO".into(),
                        message: "new session".into(),
                    }],
                })),
            },
        )
        .await;
    let logs = command.await.unwrap().unwrap();
    assert_eq!(logs.len(), 1);
    assert_eq!(logs[0].message, "new session");

    manager
        .unregister_node_stream(1, &new_tx, &db)
        .await
        .unwrap();
    assert!(!node(&db, 1).await.is_online);
    assert!(manager.get_loaded_node_ids().await.is_empty());
}

#[tokio::test]
async fn live_status_overrides_stale_database_values_and_controller_restart() {
    let db = database().await;
    let manager = NodeManager::new();
    let mut nodes = vec![node(&db, 1).await];
    let (tx, _rx) = mpsc::channel(4);
    manager
        .register_node_stream(1, tx.clone(), &db)
        .await
        .unwrap();
    assert!(!nodes[0].is_online);
    manager.apply_online_status(&mut nodes).await;
    assert!(nodes[0].is_online);

    // A Controller restart has no live sessions despite persisted online=true.
    let restarted = NodeManager::new();
    restarted.apply_online_status(&mut nodes).await;
    assert!(!nodes[0].is_online);
    restarted.sync_online_status(&db).await.unwrap();
    assert!(!node(&db, 1).await.is_online);

    manager.sync_online_status(&db).await.unwrap();
    assert!(node(&db, 1).await.is_online);
    manager.apply_online_status(&mut nodes).await;
    manager.unregister_node_stream(1, &tx, &db).await.unwrap();
    manager.apply_online_status(&mut nodes).await;
    assert!(!nodes[0].is_online);
}

#[tokio::test]
async fn closed_stream_is_offline_in_lists_and_operation_targets_without_affecting_other_nodes() {
    let db = database().await;
    let manager = NodeManager::new();
    let (first_tx, first_rx) = mpsc::channel(4);
    let (second_tx, _second_rx) = mpsc::channel(4);
    manager
        .register_node_stream(1, first_tx.clone(), &db)
        .await
        .unwrap();
    manager
        .register_node_stream(2, second_tx, &db)
        .await
        .unwrap();
    drop(first_rx);

    let mut nodes = vec![node(&db, 1).await, node(&db, 2).await];
    manager.apply_online_status(&mut nodes).await;
    assert!(!nodes[0].is_online);
    assert!(nodes[1].is_online);
    assert_eq!(manager.get_loaded_node_ids().await, vec![2]);
    manager.sync_online_status(&db).await.unwrap();
    assert!(!node(&db, 1).await.is_online);
    assert!(node(&db, 2).await.is_online);
    manager
        .unregister_node_stream(1, &first_tx, &db)
        .await
        .unwrap();
    manager
        .unregister_node_stream(1, &first_tx, &db)
        .await
        .unwrap();
    assert_eq!(manager.get_loaded_node_ids().await, vec![2]);
}

#[tokio::test]
async fn reconnect_cleanup_and_health_sync_preserve_the_current_session() {
    let db = database().await;
    let manager = NodeManager::new();
    let (mut tx, mut rx) = mpsc::channel(4);
    manager
        .register_node_stream(1, tx.clone(), &db)
        .await
        .unwrap();
    for _ in 0..20 {
        let (next_tx, next_rx) = mpsc::channel(4);
        let (registered, synced, removed) = tokio::join!(
            manager.register_node_stream(1, next_tx.clone(), &db),
            manager.sync_online_status(&db),
            manager.unregister_node_stream(1, &tx, &db),
        );
        registered.unwrap();
        synced.unwrap();
        removed.unwrap();
        assert!(node(&db, 1).await.is_online);
        assert_eq!(manager.get_loaded_node_ids().await, vec![1]);
        tx = next_tx;
        rx = next_rx;
    }
    assert!(!rx.is_closed());
    manager.unregister_node_stream(1, &tx, &db).await.unwrap();
    manager.sync_online_status(&db).await.unwrap();
    assert!(!node(&db, 1).await.is_online);
}

#[tokio::test]
async fn status_updates_preserve_node_settings_and_last_online_time_on_disconnect() {
    let db = database().await;
    let manager = NodeManager::new();
    let (tx, _rx) = mpsc::channel(4);
    manager
        .register_node_stream(1, tx.clone(), &db)
        .await
        .unwrap();
    let last_online = node(&db, 1).await.last_online_at;
    assert!(last_online.is_some());
    db.execute_unprepared("UPDATE node SET name='edited', tunnel_port=17000, tunnel_protocol='kcp', version='0.1.39' WHERE id=1")
        .await.unwrap();
    manager.unregister_node_stream(1, &tx, &db).await.unwrap();
    manager.sync_online_status(&db).await.unwrap();
    let node = node(&db, 1).await;
    assert!(!node.is_online);
    assert_eq!(node.last_online_at, last_online);
    assert_eq!(node.name, "edited");
    assert_eq!(node.tunnel_port, 17000);
    assert_eq!(node.tunnel_protocol, "kcp");
    assert_eq!(node.version.as_deref(), Some("0.1.39"));
}
