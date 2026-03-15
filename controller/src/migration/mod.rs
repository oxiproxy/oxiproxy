use sea_orm::{Database, DatabaseConnection};
use sea_orm_migration::prelude::*;
use std::fs::create_dir_all;
use std::{fs, path};
use tokio::sync::OnceCell;

mod m20250119_000001_init;
mod m20250126_000001_add_traffic_limit;
mod m20250126_000002_create_system_config;
mod m20250128_000001_add_client_traffic_limit;
mod m20250203_000001_add_kcp_config;
mod m20250204_000001_add_kcp_enabled;
mod m20250204_000002_remove_kcp_port;
mod m20250213_000001_add_node;
mod m20250214_000001_add_node_tunnel_fields;
mod m20250214_000002_add_proxy_node_id;
mod m20250214_000003_create_user_node;
mod m20250215_000001_remove_unused_configs;
mod m20250224_000001_add_traffic_quota;
mod m20250224_000002_add_client_user_id;
mod m20250224_000003_remove_client_node_id;
mod m20250224_000004_remove_traffic_limits;
mod m20260224_000001_add_node_type;
mod m20260224_000002_add_port_limits;
mod m20260224_000003_add_node_public_ip;
mod m20260224_000004_create_subscription;
mod m20260225_000001_add_controller_configs;
mod m20260225_000002_remove_unused_quic_configs;
mod m20260225_000003_add_grpc_tls_config;
mod m20260226_000001_add_subscription_port_limit;
mod m20260226_000002_add_cert_content_config;
mod m20260227_000001_add_web_tls_config;
mod m20260228_000001_add_client_public_ip;
mod m20260228_000002_add_enable_registration_config;
mod m20260228_000003_add_node_limits;
mod m20260301_000001_add_proxy_group_id;
mod m20260301_000002_add_client_region;
mod m20260301_000003_add_user_quotas;
mod m20260301_000004_add_subscription_quotas;
mod m20260301_000005_add_subscription_quota_snapshots;
mod m20260302_000001_add_version_fields;
mod m20260313_000001_add_default_quota_config;
mod m20260315_000001_add_node_certificates;

pub struct Migrator;

#[async_trait::async_trait]
impl MigratorTrait for Migrator {
    fn migrations() -> Vec<Box<dyn MigrationTrait>> {
        vec![
            Box::new(m20250119_000001_init::Migration),
            Box::new(m20250126_000001_add_traffic_limit::Migration),
            Box::new(m20250126_000002_create_system_config::Migration),
            Box::new(m20250128_000001_add_client_traffic_limit::Migration),
            Box::new(m20250203_000001_add_kcp_config::Migration),
            Box::new(m20250204_000001_add_kcp_enabled::Migration),
            Box::new(m20250204_000002_remove_kcp_port::Migration),
            Box::new(m20250213_000001_add_node::Migration),
            Box::new(m20250214_000001_add_node_tunnel_fields::Migration),
            Box::new(m20250214_000002_add_proxy_node_id::Migration),
            Box::new(m20250214_000003_create_user_node::Migration),
            Box::new(m20250215_000001_remove_unused_configs::Migration),
            Box::new(m20250224_000001_add_traffic_quota::Migration),
            Box::new(m20250224_000002_add_client_user_id::Migration),
            Box::new(m20250224_000003_remove_client_node_id::Migration),
            Box::new(m20250224_000004_remove_traffic_limits::Migration),
            Box::new(m20260224_000001_add_node_type::Migration),
            Box::new(m20260224_000002_add_port_limits::Migration),
            Box::new(m20260224_000003_add_node_public_ip::Migration),
            Box::new(m20260224_000004_create_subscription::Migration),
            Box::new(m20260225_000001_add_controller_configs::Migration),
            Box::new(m20260225_000002_remove_unused_quic_configs::Migration),
            Box::new(m20260225_000003_add_grpc_tls_config::Migration),
            Box::new(m20260226_000001_add_subscription_port_limit::Migration),
            Box::new(m20260226_000002_add_cert_content_config::Migration),
            Box::new(m20260227_000001_add_web_tls_config::Migration),
            Box::new(m20260228_000001_add_client_public_ip::Migration),
            Box::new(m20260228_000002_add_enable_registration_config::Migration),
            Box::new(m20260228_000003_add_node_limits::Migration),
            Box::new(m20260301_000001_add_proxy_group_id::Migration),
            Box::new(m20260301_000002_add_client_region::Migration),
            Box::new(m20260301_000003_add_user_quotas::Migration),
            Box::new(m20260301_000004_add_subscription_quotas::Migration),
            Box::new(m20260301_000005_add_subscription_quota_snapshots::Migration),
            Box::new(m20260302_000001_add_version_fields::Migration),
            Box::new(m20260313_000001_add_default_quota_config::Migration),
            Box::new(m20260315_000001_add_node_certificates::Migration),
        ]
    }
}

static DATABASE_CONNECTION: OnceCell<DatabaseConnection> = OnceCell::const_new();

pub async fn get_connection() -> &'static DatabaseConnection {
    DATABASE_CONNECTION.get_or_init(init_sqlite).await
}

pub async fn init_sqlite() -> DatabaseConnection {
    let path = path::Path::new("data/oxiproxy.db");
    if !path.exists() {
        if let Some(parent) = path.parent() {
            create_dir_all(parent).unwrap();
        }
        fs::write(path, "").unwrap();
    }
    let db = Database::connect("sqlite://data/oxiproxy.db")
        .await
        .expect("failed to connect sqlite");

    db
}
