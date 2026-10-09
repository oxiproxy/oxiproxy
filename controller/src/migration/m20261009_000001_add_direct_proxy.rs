use sea_orm::{ConnectionTrait, Statement, TransactionTrait};
use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

// Rebuild both tables in one transaction, keeping foreign keys enabled. Backing
// up traffic_daily before dropping it prevents ON DELETE CASCADE from erasing it.
async fn rebuild(manager: &SchemaManager<'_>, direct: bool) -> Result<(), DbErr> {
    let db = manager.get_connection();
    let objects = db.query_all(Statement::from_string(db.get_database_backend(),
        "SELECT sql FROM sqlite_master WHERE tbl_name IN ('proxy','traffic_daily') AND type IN ('index','trigger') AND sql IS NOT NULL".to_owned())).await?;
    let txn = db.begin().await?;
    let sequences = txn
        .query_all(Statement::from_string(
            db.get_database_backend(),
            "SELECT name,seq FROM sqlite_sequence WHERE name IN ('proxy','traffic_daily')"
                .to_owned(),
        ))
        .await?;
    txn.execute_unprepared(
        "CREATE TEMP TABLE direct_proxy_daily_backup AS SELECT * FROM traffic_daily",
    )
    .await?;
    txn.execute_unprepared("DROP TABLE traffic_daily").await?;
    let client = if direct { "TEXT" } else { "TEXT NOT NULL" };
    let extra = if direct {
        ", user_id BIGINT REFERENCES user(id) ON DELETE CASCADE, upstream_url TEXT NOT NULL DEFAULT '',
         CHECK ((upstream_url = '' AND client_id IS NOT NULL) OR
                (upstream_url != '' AND client_id IS NULL AND user_id IS NOT NULL AND node_id IS NOT NULL AND proxy_type IN ('http','https') AND group_id IS NULL))"
    } else {
        ""
    };
    txn.execute_unprepared(&format!(
        "CREATE TABLE proxy_direct_new (
            id INTEGER PRIMARY KEY AUTOINCREMENT, name TEXT NOT NULL, proxy_type TEXT NOT NULL,
            local_ip TEXT NOT NULL, local_port INTEGER NOT NULL, remote_port INTEGER NOT NULL,
            enabled BOOLEAN NOT NULL, total_bytes_sent BIGINT NOT NULL DEFAULT 0,
            total_bytes_received BIGINT NOT NULL DEFAULT 0, created_at DATETIME NOT NULL,
            updated_at DATETIME NOT NULL, client_id {client} REFERENCES client(id) ON DELETE CASCADE,
            node_id BIGINT, group_id TEXT, domain TEXT NOT NULL DEFAULT '' {extra})"
    )).await?;
    let columns = "id,name,proxy_type,local_ip,local_port,remote_port,enabled,total_bytes_sent,total_bytes_received,created_at,updated_at,client_id,node_id,group_id,domain";
    txn.execute_unprepared(&format!(
        "INSERT INTO proxy_direct_new ({columns}) SELECT {columns} FROM proxy"
    ))
    .await?;
    txn.execute_unprepared("DROP TABLE proxy").await?;
    txn.execute_unprepared("ALTER TABLE proxy_direct_new RENAME TO proxy")
        .await?;
    let daily_client = if direct { "BIGINT" } else { "BIGINT NOT NULL" };
    txn.execute_unprepared(&format!(
        "CREATE TABLE traffic_daily (
            id INTEGER PRIMARY KEY AUTOINCREMENT, proxy_id BIGINT NOT NULL REFERENCES proxy(id) ON DELETE CASCADE,
            client_id {daily_client} REFERENCES client(id) ON DELETE CASCADE,
            bytes_sent BIGINT NOT NULL DEFAULT 0, bytes_received BIGINT NOT NULL DEFAULT 0,
            date TEXT NOT NULL, created_at DATETIME NOT NULL, updated_at DATETIME NOT NULL)"
    )).await?;
    txn.execute_unprepared("INSERT INTO traffic_daily SELECT * FROM direct_proxy_daily_backup")
        .await?;
    txn.execute_unprepared("DROP TABLE direct_proxy_daily_backup")
        .await?;
    for object in objects {
        txn.execute_unprepared(&object.try_get::<String>("", "sql")?)
            .await?;
    }
    for sequence in sequences {
        let name: String = sequence.try_get("", "name")?;
        let value: i64 = sequence.try_get("", "seq")?;
        txn.execute_unprepared(&format!(
            "UPDATE sqlite_sequence SET seq = MAX(seq, {value}) WHERE name = '{name}'"
        ))
        .await?;
    }
    txn.commit().await
}

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        rebuild(manager, true).await
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let db = manager.get_connection();
        let direct = db
            .query_one(Statement::from_string(
                db.get_database_backend(),
                "SELECT id FROM proxy WHERE client_id IS NULL LIMIT 1".to_owned(),
            ))
            .await?;
        if direct.is_some() {
            return Err(DbErr::Custom(
                "请先删除公网直连代理，再回滚数据库迁移".into(),
            ));
        }
        rebuild(manager, false).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::migration::Migrator;
    use sea_orm::Database;

    #[tokio::test]
    async fn migration_preserves_existing_proxies_daily_data_and_route_constraints() {
        let db = Database::connect("sqlite::memory:").await.unwrap();
        for migration in Migrator::migrations() {
            if migration.name() == Migration.name() {
                break;
            }
            migration.up(&SchemaManager::new(&db)).await.unwrap();
        }
        db.execute_unprepared("INSERT INTO client (id,name,token,is_online,total_bytes_sent,total_bytes_received,created_at,updated_at) VALUES (1,'client','test',0,0,0,CURRENT_TIMESTAMP,CURRENT_TIMESTAMP)").await.unwrap();
        db.execute_unprepared("INSERT INTO user (id,username,password_hash,is_admin,total_bytes_sent,total_bytes_received,created_at,updated_at) VALUES (1,'owner','test',0,0,0,CURRENT_TIMESTAMP,CURRENT_TIMESTAMP)").await.unwrap();
        db.execute_unprepared("INSERT INTO proxy (id,name,proxy_type,local_ip,local_port,remote_port,enabled,created_at,updated_at,client_id,node_id,domain) VALUES (1,'old','http','127.0.0.1',80,8080,1,CURRENT_TIMESTAMP,CURRENT_TIMESTAMP,'1',1,'old.test')").await.unwrap();
        db.execute_unprepared("INSERT INTO traffic_daily (proxy_id,client_id,bytes_sent,date,created_at,updated_at) VALUES (1,1,123,'2026-10-09',CURRENT_TIMESTAMP,CURRENT_TIMESTAMP)").await.unwrap();
        let manager = SchemaManager::new(&db);
        Migration.up(&manager).await.unwrap();
        let daily = db
            .query_one(Statement::from_string(
                db.get_database_backend(),
                "SELECT bytes_sent FROM traffic_daily".to_owned(),
            ))
            .await
            .unwrap()
            .unwrap();
        assert_eq!(daily.try_get::<i64>("", "bytes_sent").unwrap(), 123);
        db.execute_unprepared("INSERT INTO proxy (id,name,proxy_type,local_ip,local_port,remote_port,enabled,created_at,updated_at,client_id,node_id,domain,user_id,upstream_url) VALUES (2,'direct','http','',0,8080,1,CURRENT_TIMESTAMP,CURRENT_TIMESTAMP,NULL,1,'direct.test',1,'https://origin.test/')").await.unwrap();
        db.execute_unprepared("INSERT INTO traffic_daily (proxy_id,client_id,bytes_sent,date,created_at,updated_at) VALUES (2,NULL,456,'2026-10-09',CURRENT_TIMESTAMP,CURRENT_TIMESTAMP)").await.unwrap();
        assert!(db
            .execute_unprepared("UPDATE proxy SET domain='old.test' WHERE id=2")
            .await
            .is_err());
        assert!(Migration.down(&manager).await.is_err());
        assert!(db
            .query_all(Statement::from_string(
                db.get_database_backend(),
                "PRAGMA foreign_key_check".to_owned()
            ))
            .await
            .unwrap()
            .is_empty());
        db.execute_unprepared("DELETE FROM proxy WHERE id=2")
            .await
            .unwrap();
        Migration.down(&manager).await.unwrap();
        assert!(db
            .query_all(Statement::from_string(
                db.get_database_backend(),
                "PRAGMA foreign_key_check".to_owned()
            ))
            .await
            .unwrap()
            .is_empty());
    }
}
