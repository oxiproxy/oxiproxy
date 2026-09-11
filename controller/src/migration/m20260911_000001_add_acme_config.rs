use sea_orm_migration::prelude::*;
#[derive(DeriveMigrationName)] pub struct Migration;
#[async_trait::async_trait]
impl MigrationTrait for Migration {
 async fn up(&self, m:&SchemaManager)->Result<(),DbErr>{
  let c=m.get_connection();
  for (k,v,d) in [("acme_enabled","false","Enable Let's Encrypt ACME renewal"),("acme_domains","","Comma separated certificate domains"),("acme_email","","ACME account email"),("acme_staging","false","Use ACME staging endpoint")]{
   let _=c.execute_unprepared(&format!("INSERT INTO system_configs (key,value,description,value_type,created_at,updated_at) VALUES ('{}','{}','{}','string',datetime('now'),datetime('now')) ON CONFLICT(key) DO NOTHING",k,v,d.replace("'", "''"))).await?;
  }
  Ok(())
 }
 async fn down(&self,m:&SchemaManager)->Result<(),DbErr>{m.get_connection().execute_unprepared("DELETE FROM system_configs WHERE key LIKE 'acme_%'").await.map(|_|())}
}
