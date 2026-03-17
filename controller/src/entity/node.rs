use sea_orm::entity::prelude::*;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, DeriveEntityModel, Serialize, Deserialize)]
#[sea_orm(table_name = "node")]
pub struct Model {
    #[sea_orm(primary_key)]
    pub id: i64,
    pub name: String,
    pub url: String,
    pub secret: String,
    #[serde(rename = "isOnline")]
    pub is_online: bool,
    pub region: Option<String>,
    #[serde(rename = "publicIp")]
    pub public_ip: Option<String>,
    pub description: Option<String>,
    #[serde(rename = "tunnelAddr")]
    pub tunnel_addr: String,
    #[serde(rename = "tunnelPort")]
    pub tunnel_port: i32,
    #[serde(rename = "tunnelProtocol")]
    pub tunnel_protocol: String,
    #[serde(rename = "kcpConfig")]
    pub kcp_config: Option<String>,
    #[serde(rename = "nodeType")]
    pub node_type: String,
    #[serde(rename = "maxProxyCount")]
    pub max_proxy_count: Option<i32>,
    #[serde(rename = "allowedPortRange")]
    pub allowed_port_range: Option<String>,
    #[serde(rename = "trafficQuotaGb")]
    pub traffic_quota_gb: Option<f64>,
    #[serde(rename = "trafficResetCycle")]
    pub traffic_reset_cycle: String,
    #[serde(rename = "totalBytesSent")]
    pub total_bytes_sent: i64,
    #[serde(rename = "totalBytesReceived")]
    pub total_bytes_received: i64,
    #[serde(rename = "lastResetAt")]
    pub last_reset_at: Option<DateTime>,
    #[serde(rename = "isTrafficExceeded")]
    pub is_traffic_exceeded: bool,
    #[serde(rename = "speedLimit")]
    pub speed_limit: Option<i64>,
    pub version: Option<String>,
    #[serde(rename = "lastOnlineAt")]
    pub last_online_at: Option<DateTime>,
    pub created_at: DateTime,
    pub updated_at: DateTime,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {
    #[sea_orm(has_many = "super::user_node::Entity")]
    UserNodes,
}

impl Related<super::user_node::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::UserNodes.def()
    }
}

impl ActiveModelBehavior for ActiveModel {}
