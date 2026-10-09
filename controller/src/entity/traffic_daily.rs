use sea_orm::entity::prelude::*;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, DeriveEntityModel, Serialize, Deserialize)]
#[sea_orm(table_name = "traffic_daily")]
pub struct Model {
    #[sea_orm(primary_key)]
    pub id: i64,
    pub proxy_id: i64,
    pub client_id: Option<i64>,
    pub bytes_sent: i64,
    pub bytes_received: i64,
    pub date: String, // 格式: YYYY-MM-DD
    pub created_at: DateTime,
    pub updated_at: DateTime,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {
    #[sea_orm(
        belongs_to = "super::proxy::Entity",
        from = "Column::ProxyId",
        to = "super::proxy::Column::Id"
    )]
    Proxy,
    #[sea_orm(
        belongs_to = "super::client::Entity",
        from = "Column::ClientId",
        to = "super::client::Column::Id"
    )]
    Client,
}

impl Related<super::proxy::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::Proxy.def()
    }
}

impl Related<super::client::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::Client.def()
    }
}

impl ActiveModelBehavior for ActiveModel {}
