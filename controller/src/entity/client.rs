use sea_orm::entity::prelude::*;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, DeriveEntityModel, Serialize, Deserialize)]
#[sea_orm(table_name = "client")]
pub struct Model {
    #[sea_orm(primary_key)]
    pub id: i64,
    pub name: String,
    pub token: String,
    pub is_online: bool,
    #[serde(rename = "publicIp")]
    pub public_ip: Option<String>,
    pub region: Option<String>,
    #[serde(rename = "totalBytesSent")]
    pub total_bytes_sent: i64,
    #[serde(rename = "totalBytesReceived")]
    pub total_bytes_received: i64,
    #[serde(rename = "trafficResetCycle")]
    pub traffic_reset_cycle: String,
    #[serde(rename = "lastResetAt")]
    pub last_reset_at: Option<DateTime>,
    #[serde(rename = "isTrafficExceeded")]
    pub is_traffic_exceeded: bool,
    #[serde(rename = "trafficQuotaGb")]
    pub traffic_quota_gb: Option<f64>,
    #[serde(rename = "userId")]
    pub user_id: Option<i64>,
    pub version: Option<String>,
    #[serde(rename = "lastOnlineAt")]
    pub last_online_at: Option<DateTime>,
    pub created_at: DateTime,
    pub updated_at: DateTime,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {
    #[sea_orm(
        belongs_to = "super::user::Entity",
        from = "Column::UserId",
        to = "super::user::Column::Id"
    )]
    User,
}

impl Related<super::user::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::User.def()
    }
}

impl ActiveModelBehavior for ActiveModel {}
