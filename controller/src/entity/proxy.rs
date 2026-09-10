use sea_orm::entity::prelude::*;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, DeriveEntityModel, Serialize, Deserialize)]
#[sea_orm(table_name = "proxy")]
pub struct Model {
    #[sea_orm(primary_key)]
    pub id: i64,
    pub client_id: String,
    pub name: String,
    #[serde(rename = "type")]
    pub proxy_type: String,
    pub domain: String,
    #[serde(rename = "localIP")]
    pub local_ip: String,
    #[serde(rename = "localPort")]
    pub local_port: u16,
    #[serde(rename = "remotePort")]
    pub remote_port: u16,
    pub enabled: bool,
    #[serde(rename = "nodeId")]
    pub node_id: Option<i64>,
    #[serde(rename = "groupId")]
    pub group_id: Option<String>,
    #[serde(rename = "totalBytesSent")]
    pub total_bytes_sent: i64,
    #[serde(rename = "totalBytesReceived")]
    pub total_bytes_received: i64,
    pub created_at: DateTime,
    pub updated_at: DateTime,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {
    #[sea_orm(
        belongs_to = "super::client::Entity",
        from = "Column::ClientId",
        to = "super::client::Column::Id"
    )]
    Client,
}

impl ActiveModelBehavior for ActiveModel {}
