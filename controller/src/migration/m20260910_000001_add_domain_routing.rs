use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let db = manager.get_connection();
        db.execute_unprepared("ALTER TABLE proxy ADD COLUMN domain TEXT NOT NULL DEFAULT ''")
            .await?;
        // Database enforcement also covers concurrent requests and group enable operations.
        for (name, event) in [
            ("proxy_route_insert", "INSERT"),
            (
                "proxy_route_update",
                "UPDATE OF node_id, remote_port, proxy_type, domain, enabled",
            ),
        ] {
            db.execute_unprepared(&format!(
                "CREATE TRIGGER {name} BEFORE {event} ON proxy
                 WHEN NEW.enabled = 1 AND EXISTS (
                   SELECT 1 FROM proxy p WHERE p.id != NEW.id AND p.enabled = 1
                   AND p.node_id IS NEW.node_id AND p.remote_port = NEW.remote_port
                   AND (NEW.proxy_type NOT IN ('http','https') OR p.proxy_type != NEW.proxy_type OR p.domain = NEW.domain)
                 ) BEGIN SELECT RAISE(ABORT, 'proxy port/domain conflict'); END"
            )).await?;
        }
        Ok(())
    }
    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let db = manager.get_connection();
        db.execute_unprepared("DROP TRIGGER IF EXISTS proxy_route_insert")
            .await?;
        db.execute_unprepared("DROP TRIGGER IF EXISTS proxy_route_update")
            .await?;
        db.execute_unprepared("ALTER TABLE proxy DROP COLUMN domain")
            .await?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sea_orm::Database;

    #[tokio::test]
    async fn route_constraints_cover_insert_update_enable_and_node_isolation() {
        let db = Database::connect("sqlite::memory:").await.unwrap();
        db.execute_unprepared("CREATE TABLE proxy (id INTEGER PRIMARY KEY, node_id INTEGER, remote_port INTEGER, proxy_type TEXT, enabled INTEGER)").await.unwrap();
        db.execute_unprepared("INSERT INTO proxy VALUES (1, 1, 8080, 'tcp', 1)")
            .await
            .unwrap();
        let manager = SchemaManager::new(&db);
        Migration.up(&manager).await.unwrap();
        let insert = |id, node: &str, port, kind: &str, domain: &str, enabled| {
            format!(
            "INSERT INTO proxy (id,node_id,remote_port,proxy_type,domain,enabled) VALUES ({id},{node},{port},'{kind}','{domain}',{enabled})")
        };
        for sql in [
            insert(2, "1", 80, "http", "a.test", 1),
            insert(3, "1", 80, "http", "b.test", 1),
            insert(4, "2", 80, "http", "a.test", 1),
            insert(5, "1", 80, "http", "a.test", 0),
            insert(6, "NULL", 443, "https", "a.test", 1),
        ] {
            db.execute_unprepared(&sql).await.unwrap();
        }
        for sql in [
            insert(7, "1", 80, "http", "a.test", 1),
            insert(7, "1", 80, "tcp", "", 1),
            insert(7, "1", 80, "https", "c.test", 1),
            insert(7, "NULL", 443, "https", "a.test", 1),
            insert(7, "1", 8080, "http", "c.test", 1),
            "UPDATE proxy SET enabled=1 WHERE id=5".into(),
            "UPDATE proxy SET domain='a.test' WHERE id=3".into(),
            "UPDATE proxy SET proxy_type='tcp' WHERE id=3".into(),
            "UPDATE proxy SET node_id=1 WHERE id=4".into(),
        ] {
            assert!(db.execute_unprepared(&sql).await.is_err(), "{sql}");
        }
        db.execute_unprepared("UPDATE proxy SET enabled=0 WHERE id=2")
            .await
            .unwrap();
        db.execute_unprepared("UPDATE proxy SET enabled=1 WHERE id=5")
            .await
            .unwrap();
        Migration.down(&manager).await.unwrap();
    }
}
