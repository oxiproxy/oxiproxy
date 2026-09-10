//! 本地认证提供者（Controller 直接查询数据库）

use anyhow::Result;
use async_trait::async_trait;
use sea_orm::{ActiveModelTrait, ColumnTrait, EntityTrait, QueryFilter, Set};
use tracing::debug;

use common::protocol::auth::{
    ClientAuthProvider, TrafficLimitResponse, ValidateTokenResponse,
};
use common::protocol::control::ProxyConfig;

use crate::entity::{Client, Proxy, User, client, proxy};
use crate::migration::get_connection;

pub struct LocalControllerAuthProvider;

impl LocalControllerAuthProvider {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl ClientAuthProvider for LocalControllerAuthProvider {
    async fn validate_token(&self, token: &str) -> Result<ValidateTokenResponse> {
        let db = get_connection().await;

        let client = match Client::find()
            .filter(client::Column::Token.eq(token))
            .one(db)
            .await?
        {
            Some(c) => c,
            None => {
                return Ok(ValidateTokenResponse {
                    client_id: 0,
                    client_name: String::new(),
                    allowed: false,
                    reject_reason: Some("无效的 token".to_string()),
                });
            }
        };

        let client_id = client.id;
        let client_name = client.name.clone();

        // 检查流量限制（通过 client.user_id → User）
        if let Some(user_id) = client.user_id {
            if let Ok(Some(user)) = User::find_by_id(user_id).one(db).await {
                if user.is_traffic_exceeded {
                    return Ok(ValidateTokenResponse {
                        client_id,
                        client_name,
                        allowed: false,
                        reject_reason: Some(format!(
                            "用户 {} (#{}) 流量已超限",
                            user.username, user.id
                        )),
                    });
                }
            }
        }

        Ok(ValidateTokenResponse {
            client_id,
            client_name,
            allowed: true,
            reject_reason: None,
        })
    }

    async fn set_client_online(&self, client_id: i64, online: bool) -> Result<()> {
        let db = get_connection().await;
        if let Some(client) = Client::find_by_id(client_id).one(db).await? {
            let mut client_active: client::ActiveModel = client.into();
            client_active.is_online = Set(online);
            debug!("更新客户端 #{} 状态: online={}", client_id, online);
            let _ = client_active.update(db).await;
        }
        Ok(())
    }

    async fn check_traffic_limit(&self, client_id: i64) -> Result<TrafficLimitResponse> {
        let db = get_connection().await;

        // 查找 client 的 user_id，然后检查用户流量限制
        let client = match Client::find_by_id(client_id).one(db).await? {
            Some(c) => c,
            None => {
                return Ok(TrafficLimitResponse {
                    exceeded: false,
                    reason: None,
                });
            }
        };

        if let Some(user_id) = client.user_id {
            if let Ok(Some(user)) = User::find_by_id(user_id).one(db).await {
                if user.is_traffic_exceeded {
                    return Ok(TrafficLimitResponse {
                        exceeded: true,
                        reason: Some(format!(
                            "用户 {} (#{}) 流量已超限",
                            user.username, user.id
                        )),
                    });
                }
            }
        }

        Ok(TrafficLimitResponse {
            exceeded: false,
            reason: None,
        })
    }

    async fn get_client_proxies(&self, client_id: i64) -> Result<Vec<ProxyConfig>> {
        let db = get_connection().await;
        let client_id_str = client_id.to_string();

        let proxies = Proxy::find()
            .filter(proxy::Column::ClientId.eq(&client_id_str))
            .filter(proxy::Column::Enabled.eq(true))
            .all(db)
            .await?;

        Ok(proxies
            .into_iter()
            .map(|p| ProxyConfig {
                proxy_id: p.id,
                client_id: p.client_id,
                name: p.name,
                proxy_type: p.proxy_type,
                    domain: p.domain,
                local_ip: p.local_ip,
                local_port: p.local_port,
                remote_port: p.remote_port,
                enabled: p.enabled,
            })
            .collect())
    }
}
