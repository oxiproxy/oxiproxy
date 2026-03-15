use anyhow::Result;
use chrono::{Datelike, Utc};
use sea_orm::{ActiveModelTrait, ColumnTrait, DatabaseConnection, EntityTrait, QueryFilter, Set};
use tracing::{info};

use crate::entity::{client, node, user, Client, User};

/// GB 转字节
pub fn gb_to_bytes(gb: f64) -> i64 {
    (gb * 1024.0 * 1024.0 * 1024.0) as i64
}

/// 字节转 GB
pub fn bytes_to_gb(bytes: i64) -> f64 {
    bytes as f64 / 1024.0 / 1024.0 / 1024.0
}

/// 计算用户剩余流量（GB）
pub fn calculate_user_remaining_quota(user: &user::Model) -> Option<f64> {
    user.traffic_quota_gb.map(|quota| {
        let used_gb = bytes_to_gb(user.total_bytes_sent + user.total_bytes_received);
        (quota - used_gb).max(0.0)
    })
}

/// 计算客户端剩余流量（GB）
pub fn calculate_client_remaining_quota(client: &client::Model) -> Option<f64> {
    client.traffic_quota_gb.map(|quota| {
        let used_gb = bytes_to_gb(client.total_bytes_sent + client.total_bytes_received);
        (quota - used_gb).max(0.0)
    })
}

/// 判断是否需要重置流量
pub fn should_reset_traffic(user: &user::Model) -> bool {
    let now = Utc::now().naive_utc();

    let last_reset = match user.last_reset_at {
        None => return true, // 从未重置过，需要初始化
        Some(t) => t,
    };

    match user.traffic_reset_cycle.as_str() {
        "daily" => {
            now.date() > last_reset.date()
        },
        "monthly" => {
            now.year() > last_reset.year() ||
            (now.year() == last_reset.year() && now.month() > last_reset.month())
        },
        _ => false,
    }
}

/// 重置用户流量统计
pub async fn reset_user_traffic(user_id: i64, db: &DatabaseConnection) -> Result<()> {
    let user = match User::find_by_id(user_id).one(db).await? {
        Some(u) => u,
        None => return Ok(()),
    };

    let mut user_active: user::ActiveModel = user.into();
    user_active.total_bytes_sent = Set(0);
    user_active.total_bytes_received = Set(0);
    user_active.is_traffic_exceeded = Set(false);
    user_active.last_reset_at = Set(Some(Utc::now().naive_utc()));
    user_active.updated_at = Set(Utc::now().naive_utc());

    user_active.update(db).await?;
    info!("✅ 用户 #{} 流量已重置", user_id);

    Ok(())
}

/// 检查用户流量是否超限（支持配额模式）
pub async fn check_user_traffic_limit(user_id: i64, db: &DatabaseConnection) -> Result<(bool, String)> {
    let user = match User::find_by_id(user_id).one(db).await? {
        Some(u) => u,
        None => return Ok((false, String::new())),
    };

    if should_reset_traffic(&user) {
        reset_user_traffic(user_id, db).await?;
        return Ok((false, String::new()));
    }

    // 获取最终配额（套餐配额 + 用户直接配额）
    let (final_traffic_quota_gb, _, _, _) = crate::subscription_quota::get_user_final_quota(
        user_id,
        user.traffic_quota_gb,
        user.max_port_count,
        user.max_node_count,
        user.max_client_count,
        db,
    ).await?;

    // 检查配额模式
    if let Some(quota_gb) = final_traffic_quota_gb {
        let total_used = user.total_bytes_sent + user.total_bytes_received;
        let quota_bytes = gb_to_bytes(quota_gb);

        if total_used >= quota_bytes {
            let reason = format!(
                "流量配额已用尽: {:.2} GB / {:.2} GB",
                bytes_to_gb(total_used),
                quota_gb
            );
            return Ok((true, reason));
        }
    }

    Ok((false, String::new()))
}

// ============== 节点（Client）级别流量限制 ==============

/// 判断节点是否需要重置流量
pub fn should_reset_client_traffic(client: &client::Model) -> bool {
    let now = Utc::now().naive_utc();

    let last_reset = match client.last_reset_at {
        None => return true,
        Some(t) => t,
    };

    match client.traffic_reset_cycle.as_str() {
        "daily" => {
            now.date() > last_reset.date()
        },
        "monthly" => {
            now.year() > last_reset.year() ||
            (now.year() == last_reset.year() && now.month() > last_reset.month())
        },
        _ => false,
    }
}

/// 重置节点流量统计
pub async fn reset_client_traffic(client_id: i64, db: &DatabaseConnection) -> Result<()> {
    let client_model = match Client::find_by_id(client_id).one(db).await? {
        Some(c) => c,
        None => return Ok(()),
    };

    let mut client_active: client::ActiveModel = client_model.into();
    client_active.total_bytes_sent = Set(0);
    client_active.total_bytes_received = Set(0);
    client_active.is_traffic_exceeded = Set(false);
    client_active.last_reset_at = Set(Some(Utc::now().naive_utc()));
    client_active.updated_at = Set(Utc::now().naive_utc());

    client_active.update(db).await?;
    info!("✅ 节点 #{} 流量已重置", client_id);

    Ok(())
}

/// 检查节点流量是否超限（支持配额模式）
pub async fn check_client_traffic_limit(client_id: i64, db: &DatabaseConnection) -> Result<(bool, String)> {
    let client_model = match Client::find_by_id(client_id).one(db).await? {
        Some(c) => c,
        None => return Ok((false, String::new())),
    };

    if should_reset_client_traffic(&client_model) {
        reset_client_traffic(client_id, db).await?;
        return Ok((false, String::new()));
    }

    // 检查配额模式
    if let Some(quota_gb) = client_model.traffic_quota_gb {
        let total_used = client_model.total_bytes_sent + client_model.total_bytes_received;
        let quota_bytes = gb_to_bytes(quota_gb);

        if total_used >= quota_bytes {
            let reason = format!(
                "流量配额已用尽: {:.2} GB / {:.2} GB",
                bytes_to_gb(total_used),
                quota_gb
            );
            return Ok((true, reason));
        }
    }

    Ok((false, String::new()))
}

/// 检查用户是否有足够的配额分配给客户端
pub async fn check_user_quota_allocation(user_id: i64, additional_quota_gb: f64, db: &DatabaseConnection) -> Result<(bool, String)> {
    let user = match User::find_by_id(user_id).one(db).await? {
        Some(u) => u,
        None => return Ok((false, "用户不存在".to_string())),
    };

    // 获取最终配额（套餐配额 + 用户直接配额）
    let (final_traffic_quota_gb, _, _, _) = crate::subscription_quota::get_user_final_quota(
        user_id,
        user.traffic_quota_gb,
        user.max_port_count,
        user.max_node_count,
        user.max_client_count,
        db,
    ).await?;

    // 如果用户没有配额限制，允许分配
    let user_quota_gb = match final_traffic_quota_gb {
        Some(q) => q,
        None => return Ok((true, String::new())),
    };

    // 计算用户已使用的流量
    let user_used_gb = bytes_to_gb(user.total_bytes_sent + user.total_bytes_received);

    // 查询用户所有客户端已分配的配额总和（直接通过 client.user_id）
    let user_clients = Client::find()
        .filter(client::Column::UserId.eq(user_id))
        .all(db)
        .await?;

    let mut total_allocated_gb = 0.0;
    for client in user_clients {
        if let Some(quota) = client.traffic_quota_gb {
            total_allocated_gb += quota;
        }
    }

    // 检查：已使用 + 已分配 + 新分配 <= 用户配额
    let total_needed = user_used_gb + total_allocated_gb + additional_quota_gb;

    if total_needed > user_quota_gb {
        let available = (user_quota_gb - user_used_gb - total_allocated_gb).max(0.0);
        let reason = format!(
            "配额不足: 可用 {:.2} GB，需要 {:.2} GB (用户配额: {:.2} GB，已使用: {:.2} GB，已分配: {:.2} GB)",
            available,
            additional_quota_gb,
            user_quota_gb,
            user_used_gb,
            total_allocated_gb
        );
        return Ok((false, reason));
    }

    Ok((true, String::new()))
}

/// 检查节点流量是否需要重置
pub fn should_reset_node_traffic(node: &node::Model) -> bool {
    let now = Utc::now().naive_utc();

    let last_reset = match node.last_reset_at {
        None => return true,
        Some(t) => t,
    };

    match node.traffic_reset_cycle.as_str() {
        "daily" => now.date() > last_reset.date(),
        "monthly" => {
            now.year() > last_reset.year()
                || (now.year() == last_reset.year() && now.month() > last_reset.month())
        }
        _ => false,
    }
}
