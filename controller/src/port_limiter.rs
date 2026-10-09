use anyhow::{anyhow, Result};
use sea_orm::{
    ColumnTrait, Condition, DatabaseConnection, EntityTrait, PaginatorTrait, QueryFilter,
};

use crate::entity::{proxy, Proxy, User};

/// 端口范围结构
#[derive(Debug, Clone)]
pub struct PortRange {
    pub start: u16,
    pub end: u16,
}

impl PortRange {
    /// 检查端口是否在范围内
    pub fn contains(&self, port: u16) -> bool {
        port >= self.start && port <= self.end
    }
}

/// 解析端口范围字符串
/// 格式: "1000-9999,20000-30000" 或 "8080" 或 "1000-2000"
pub fn parse_port_ranges(range_str: &str) -> Result<Vec<PortRange>> {
    let mut ranges = Vec::new();

    for part in range_str.split(',') {
        let part = part.trim();
        if part.is_empty() {
            continue;
        }

        if part.contains('-') {
            // 范围格式: "1000-9999"
            let parts: Vec<&str> = part.split('-').collect();
            if parts.len() != 2 {
                return Err(anyhow!("无效的端口范围格式: {}", part));
            }

            let start: u16 = parts[0].trim().parse()
                .map_err(|_| anyhow!("无效的起始端口: {}", parts[0]))?;
            let end: u16 = parts[1].trim().parse()
                .map_err(|_| anyhow!("无效的结束端口: {}", parts[1]))?;

            if start > end {
                return Err(anyhow!("起始端口不能大于结束端口: {}-{}", start, end));
            }

            ranges.push(PortRange { start, end });
        } else {
            // 单个端口: "8080"
            let port: u16 = part.parse()
                .map_err(|_| anyhow!("无效的端口号: {}", part))?;
            ranges.push(PortRange { start: port, end: port });
        }
    }

    if ranges.is_empty() {
        return Err(anyhow!("端口范围不能为空"));
    }

    Ok(ranges)
}

/// 检查端口是否在允许的范围内
pub fn is_port_in_ranges(port: u16, ranges: &[PortRange]) -> bool {
    ranges.iter().any(|range| range.contains(port))
}

/// 验证用户端口限制
/// 返回 (是否允许, 错误信息)
pub async fn validate_user_port_limit(
    user_id: i64,
    remote_port: u16,
    db: &DatabaseConnection,
) -> Result<(bool, String)> {
    let user = match User::find_by_id(user_id).one(db).await? {
        Some(u) => u,
        None => return Ok((false, "用户不存在".to_string())),
    };

    // 检查端口范围限制
    if let Some(allowed_range_str) = &user.allowed_port_range {
        let ranges = match parse_port_ranges(allowed_range_str) {
            Ok(r) => r,
            Err(e) => {
                return Ok((false, format!("端口范围配置错误: {}", e)));
            }
        };

        if !is_port_in_ranges(remote_port, &ranges) {
            return Ok((
                false,
                format!("端口 {} 不在允许的范围内: {}", remote_port, allowed_range_str),
            ));
        }
    }

    // 检查端口数量限制（使用套餐累加配额）
    let (_, final_max_port_count, _, _) = crate::subscription_quota::get_user_final_quota(
        user_id,
        user.traffic_quota_gb,
        user.max_port_count,
        user.max_node_count,
        user.max_client_count,
        db,
    )
    .await?;

    if let Some(max_count) = final_max_port_count {
        // 查询用户所有客户端的代理数量
        let proxy_count = get_user_port_count(user_id, db).await?;

        if proxy_count >= max_count as u64 {
            return Ok((
                false,
                format!(
                    "端口数量已达上限: {} / {} (最大 {})",
                    proxy_count, max_count, max_count
                ),
            ));
        }
    }

    Ok((true, String::new()))
}

/// 获取用户当前使用的端口数量
pub async fn get_user_port_count(user_id: i64, db: &DatabaseConnection) -> Result<u64> {
    let user_clients = crate::entity::Client::find()
        .filter(crate::entity::client::Column::UserId.eq(user_id))
        .all(db)
        .await?;

    let client_ids: Vec<i64> = user_clients.iter().map(|c| c.id).collect();

    let count = Proxy::find()
        .filter(
            Condition::any()
                .add(proxy::Column::ClientId.is_in(client_ids))
                .add(
                    Condition::all()
                        .add(proxy::Column::ClientId.is_null())
                        .add(proxy::Column::UserId.eq(user_id)),
                ),
        )
        .count(db)
        .await?;

    Ok(count)
}

/// 获取用户端口限制信息
pub async fn get_user_port_limit_info(user_id: i64, db: &DatabaseConnection) -> Result<UserPortLimitInfo> {
    let user = match User::find_by_id(user_id).one(db).await? {
        Some(u) => u,
        None => return Err(anyhow!("用户不存在")),
    };

    let current_count = get_user_port_count(user_id, db).await?;

    Ok(UserPortLimitInfo {
        max_port_count: user.max_port_count,
        allowed_port_range: user.allowed_port_range.clone(),
        current_port_count: current_count,
    })
}

/// 用户端口限制信息
#[derive(Debug, Clone)]
pub struct UserPortLimitInfo {
    pub max_port_count: Option<i32>,
    pub allowed_port_range: Option<String>,
    pub current_port_count: u64,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_port_ranges() {
        // 测试单个端口
        let ranges = parse_port_ranges("8080").unwrap();
        assert_eq!(ranges.len(), 1);
        assert_eq!(ranges[0].start, 8080);
        assert_eq!(ranges[0].end, 8080);

        // 测试端口范围
        let ranges = parse_port_ranges("1000-9999").unwrap();
        assert_eq!(ranges.len(), 1);
        assert_eq!(ranges[0].start, 1000);
        assert_eq!(ranges[0].end, 9999);

        // 测试多个范围
        let ranges = parse_port_ranges("1000-9999,20000-30000").unwrap();
        assert_eq!(ranges.len(), 2);
        assert_eq!(ranges[0].start, 1000);
        assert_eq!(ranges[0].end, 9999);
        assert_eq!(ranges[1].start, 20000);
        assert_eq!(ranges[1].end, 30000);

        // 测试混合格式
        let ranges = parse_port_ranges("8080,1000-2000,3000").unwrap();
        assert_eq!(ranges.len(), 3);

        // 测试无效格式
        assert!(parse_port_ranges("invalid").is_err());
        assert!(parse_port_ranges("1000-").is_err());
        assert!(parse_port_ranges("9999-1000").is_err());
    }

    #[test]
    fn test_is_port_in_ranges() {
        let ranges = parse_port_ranges("1000-9999,20000-30000").unwrap();

        assert!(is_port_in_ranges(1000, &ranges));
        assert!(is_port_in_ranges(5000, &ranges));
        assert!(is_port_in_ranges(9999, &ranges));
        assert!(is_port_in_ranges(20000, &ranges));
        assert!(is_port_in_ranges(25000, &ranges));
        assert!(is_port_in_ranges(30000, &ranges));

        assert!(!is_port_in_ranges(999, &ranges));
        assert!(!is_port_in_ranges(10000, &ranges));
        assert!(!is_port_in_ranges(19999, &ranges));
        assert!(!is_port_in_ranges(30001, &ranges));
    }
}
