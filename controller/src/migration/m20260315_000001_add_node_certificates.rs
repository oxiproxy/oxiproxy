use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        // 创建节点证书表
        manager
            .create_table(
                Table::create()
                    .table(NodeCertificate::Table)
                    .if_not_exists()
                    .col(
                        ColumnDef::new(NodeCertificate::Id)
                            .integer()
                            .not_null()
                            .auto_increment()
                            .primary_key(),
                    )
                    .col(ColumnDef::new(NodeCertificate::NodeId).integer().not_null())
                    .col(ColumnDef::new(NodeCertificate::CertContent).text().not_null())
                    .col(ColumnDef::new(NodeCertificate::KeyContent).text().not_null())
                    .col(ColumnDef::new(NodeCertificate::Fingerprint).string().not_null())
                    .col(ColumnDef::new(NodeCertificate::CreatedAt).timestamp().not_null())
                    .col(ColumnDef::new(NodeCertificate::ExpiresAt).timestamp().not_null())
                    .foreign_key(
                        ForeignKey::create()
                            .name("fk_node_certificate_node")
                            .from(NodeCertificate::Table, NodeCertificate::NodeId)
                            .to(Node::Table, Node::Id)
                            .on_delete(ForeignKeyAction::Cascade),
                    )
                    .to_owned(),
            )
            .await?;

        // 创建索引
        manager
            .create_index(
                Index::create()
                    .name("idx_node_certificate_node_id")
                    .table(NodeCertificate::Table)
                    .col(NodeCertificate::NodeId)
                    .unique()
                    .to_owned(),
            )
            .await?;

        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .drop_table(Table::drop().table(NodeCertificate::Table).to_owned())
            .await
    }
}

#[derive(DeriveIden)]
enum NodeCertificate {
    Table,
    Id,
    NodeId,
    CertContent,
    KeyContent,
    Fingerprint,
    CreatedAt,
    ExpiresAt,
}

#[derive(DeriveIden)]
enum Node {
    Table,
    Id,
}
