use anyhow::{anyhow, Result};
use sea_orm::{ActiveModelTrait, ColumnTrait, EntityTrait, NotSet, QueryFilter, Set};
use chrono::Utc;
use base64::{Engine, engine::general_purpose::STANDARD as BASE64};

use crate::entity::{NodeCertificate, node_certificate};
use crate::migration::get_connection;
use crate::cert_generator::generate_node_certificate;

/// 证书管理器
pub struct CertificateManager;

impl CertificateManager {
    /// 获取或生成节点证书
    ///
    /// 如果节点已有证书且未过期，返回现有证书；
    /// 否则自动生成新证书并保存到数据库。
    pub async fn get_or_generate_node_certificate(
        node_id: i64,
        node_name: &str,
    ) -> Result<NodeCertificateData> {
        let db = get_connection().await;

        // 1. 检查是否已有证书
        if let Some(existing_cert) = NodeCertificate::find()
            .filter(node_certificate::Column::NodeId.eq(node_id))
            .one(db)
            .await?
        {
            // 检查证书是否过期（提前 30 天续期）
            let now = Utc::now().naive_utc();
            let expires_soon = existing_cert.expires_at - chrono::Duration::days(30);

            if now < expires_soon {
                tracing::info!("节点 #{} 使用现有证书（过期时间: {}）",
                    node_id, existing_cert.expires_at);

                // 解码证书
                let cert_pem = String::from_utf8(BASE64.decode(&existing_cert.cert_content)?)?;
                let key_pem = String::from_utf8(BASE64.decode(&existing_cert.key_content)?)?;

                return Ok(NodeCertificateData {
                    cert_pem,
                    key_pem,
                    fingerprint: existing_cert.fingerprint,
                    expires_at: existing_cert.expires_at.and_utc().timestamp(),
                });
            } else {
                tracing::info!("节点 #{} 证书即将过期，重新生成", node_id);
            }
        }

        // 2. 生成新证书
        tracing::info!("为节点 #{} ({}) 生成新证书", node_id, node_name);
        let (cert_pem, key_pem, fingerprint) = generate_node_certificate(node_id, node_name)?;

        // 3. 保存到数据库
        let now = Utc::now().naive_utc();
        let expires_at = now + chrono::Duration::days(3650); // 10 年

        let cert_model = node_certificate::ActiveModel {
            id: NotSet,
            node_id: Set(node_id),
            cert_content: Set(BASE64.encode(&cert_pem)),
            key_content: Set(BASE64.encode(&key_pem)),
            fingerprint: Set(fingerprint.clone()),
            created_at: Set(now),
            expires_at: Set(expires_at),
        };

        // 使用 upsert 逻辑：如果已存在则更新
        let result = NodeCertificate::find()
            .filter(node_certificate::Column::NodeId.eq(node_id))
            .one(db)
            .await?;

        if let Some(existing) = result {
            // 更新现有证书
            let mut active: node_certificate::ActiveModel = existing.into();
            active.cert_content = Set(BASE64.encode(&cert_pem));
            active.key_content = Set(BASE64.encode(&key_pem));
            active.fingerprint = Set(fingerprint.clone());
            active.created_at = Set(now);
            active.expires_at = Set(expires_at);
            active.update(db).await?;
        } else {
            // 插入新证书
            cert_model.insert(db).await?;
        }

        tracing::info!("✅ 节点 #{} 证书已保存（指纹: {}）", node_id, fingerprint);

        Ok(NodeCertificateData {
            cert_pem,
            key_pem,
            fingerprint,
            expires_at: expires_at.and_utc().timestamp(),
        })
    }

    /// 获取节点证书指纹（用于 Client 验证）
    pub async fn get_node_fingerprint(node_id: i64) -> Result<Option<String>> {
        let db = get_connection().await;

        let cert = NodeCertificate::find()
            .filter(node_certificate::Column::NodeId.eq(node_id))
            .one(db)
            .await?;

        Ok(cert.map(|c| c.fingerprint))
    }

    /// 删除节点证书
    pub async fn delete_node_certificate(node_id: i64) -> Result<()> {
        let db = get_connection().await;

        NodeCertificate::delete_many()
            .filter(node_certificate::Column::NodeId.eq(node_id))
            .exec(db)
            .await?;

        tracing::info!("已删除节点 #{} 的证书", node_id);
        Ok(())
    }

    /// 强制更新节点证书（用于证书轮换）
    ///
    /// 与 get_or_generate 不同，此方法总是生成新证书，
    /// 用于主动证书轮换场景。
    pub async fn force_update_node_certificate(
        node_id: i64,
        node_name: &str,
    ) -> Result<NodeCertificateData> {
        let db = get_connection().await;

        tracing::info!("🔄 强制更新节点 #{} ({}) 的证书", node_id, node_name);

        // 生成新证书
        let (cert_pem, key_pem, fingerprint) = generate_node_certificate(node_id, node_name)?;

        // 保存到数据库
        let now = Utc::now().naive_utc();
        let expires_at = now + chrono::Duration::days(3650); // 10 年

        // 使用 upsert 逻辑
        let result = NodeCertificate::find()
            .filter(node_certificate::Column::NodeId.eq(node_id))
            .one(db)
            .await?;

        if let Some(existing) = result {
            // 更新现有证书
            let mut active: node_certificate::ActiveModel = existing.into();
            active.cert_content = Set(BASE64.encode(&cert_pem));
            active.key_content = Set(BASE64.encode(&key_pem));
            active.fingerprint = Set(fingerprint.clone());
            active.created_at = Set(now);
            active.expires_at = Set(expires_at);
            active.update(db).await?;
        } else {
            // 插入新证书
            let cert_model = node_certificate::ActiveModel {
                id: NotSet,
                node_id: Set(node_id),
                cert_content: Set(BASE64.encode(&cert_pem)),
                key_content: Set(BASE64.encode(&key_pem)),
                fingerprint: Set(fingerprint.clone()),
                created_at: Set(now),
                expires_at: Set(expires_at),
            };
            cert_model.insert(db).await?;
        }

        tracing::info!("✅ 节点 #{} 证书已强制更新（指纹: {}）", node_id, fingerprint);

        Ok(NodeCertificateData {
            cert_pem,
            key_pem,
            fingerprint,
            expires_at: expires_at.and_utc().timestamp(),
        })
    }
}

/// 节点证书数据
#[derive(Debug, Clone)]
pub struct NodeCertificateData {
    pub cert_pem: String,
    pub key_pem: String,
    pub fingerprint: String,
    pub expires_at: i64,
}
