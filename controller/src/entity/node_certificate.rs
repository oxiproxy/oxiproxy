use sea_orm::entity::prelude::*;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, DeriveEntityModel, Serialize, Deserialize)]
#[sea_orm(table_name = "node_certificates")]
pub struct Model {
    #[sea_orm(primary_key)]
    pub id: i64,
    pub node_id: i64,
    /// 证书内容（PEM 格式，base64 编码）
    pub cert_content: String,
    /// 私钥内容（PEM 格式，base64 编码）
    pub key_content: String,
    /// SHA-256 指纹
    pub fingerprint: String,
    pub created_at: DateTime,
    pub expires_at: DateTime,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {
    #[sea_orm(
        belongs_to = "super::node::Entity",
        from = "Column::NodeId",
        to = "super::node::Column::Id",
        on_delete = "Cascade"
    )]
    Node,
}

impl Related<super::node::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::Node.def()
    }
}

impl ActiveModelBehavior for ActiveModel {}
