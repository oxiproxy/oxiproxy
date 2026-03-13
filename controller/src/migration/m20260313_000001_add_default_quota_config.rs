use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let configs = vec![
            ("default_traffic_quota_gb", "0", "默认用户流量配额(GB)", "number"),
            ("default_max_port_count", "0", "默认用户最大端口数", "number"),
            ("default_max_node_count", "0", "默认用户最大节点数", "number"),
            ("default_max_client_count", "0", "默认用户最大客户端数", "number"),
        ];

        for (key, value, description, value_type) in configs {
            let insert = Query::insert()
                .into_table(SystemConfig::Table)
                .columns([
                    SystemConfig::Key,
                    SystemConfig::Value,
                    SystemConfig::Description,
                    SystemConfig::ValueType,
                ])
                .values_panic([
                    key.into(),
                    value.into(),
                    description.into(),
                    value_type.into(),
                ])
                .to_owned();

            manager.exec_stmt(insert).await?;
        }

        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let keys = vec![
            "default_traffic_quota_gb",
            "default_max_port_count",
            "default_max_node_count",
            "default_max_client_count",
        ];

        for key in keys {
            let delete = Query::delete()
                .from_table(SystemConfig::Table)
                .and_where(Expr::col(SystemConfig::Key).eq(key))
                .to_owned();
            manager.exec_stmt(delete).await?;
        }
        Ok(())
    }
}

#[derive(DeriveIden)]
enum SystemConfig {
    Table,
    Key,
    Value,
    Description,
    ValueType,
}
