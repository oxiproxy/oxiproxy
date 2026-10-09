use anyhow::Result;
use sea_orm::{ColumnTrait, DatabaseConnection, EntityTrait, PaginatorTrait, QueryFilter};

use crate::entity::{proxy, Node, Proxy};
use crate::port_limiter::{is_port_in_ranges, parse_port_ranges};

/// 验证节点的代理数量、端口范围和流量限制
/// 返回 (是否允许, 错误信息)
pub async fn validate_node_proxy_limit(
    node_id: i64,
    remote_port: u16,
    db: &DatabaseConnection,
) -> Result<(bool, String)> {
    validate_node_proxy_limit_excluding(node_id, remote_port, db, 0).await
}

pub async fn validate_node_proxy_limit_excluding(
    node_id: i64,
    remote_port: u16,
    db: &DatabaseConnection,
    exclude: i64,
) -> Result<(bool, String)> {
    let node = match Node::find_by_id(node_id).one(db).await? {
        Some(n) => n,
        None => return Ok((false, "节点不存在".to_string())),
    };

    // 检查节点流量是否已超限
    if node.is_traffic_exceeded {
        return Ok((false, "该节点流量已超限，无法创建新代理".to_string()));
    }

    // 检查端口范围限制
    if let Some(ref allowed_range_str) = node.allowed_port_range {
        if !allowed_range_str.is_empty() {
            let ranges = match parse_port_ranges(allowed_range_str) {
                Ok(r) => r,
                Err(e) => {
                    return Ok((false, format!("节点端口范围配置错误: {}", e)));
                }
            };

            if !is_port_in_ranges(remote_port, &ranges) {
                return Ok((
                    false,
                    format!(
                        "端口 {} 不在节点允许的范围内: {}",
                        remote_port, allowed_range_str
                    ),
                ));
            }
        }
    }

    // 检查代理数量限制
    if let Some(max_count) = node.max_proxy_count {
        let proxy_count = Proxy::find()
            .filter(proxy::Column::NodeId.eq(node_id))
            .filter(proxy::Column::Enabled.eq(true))
            .filter(proxy::Column::Id.ne(exclude))
            .count(db)
            .await?;

        if proxy_count >= max_count as u64 {
            return Ok((
                false,
                format!("该节点代理数量已达上限: {} / {}", proxy_count, max_count),
            ));
        }
    }

    Ok((true, String::new()))
}
