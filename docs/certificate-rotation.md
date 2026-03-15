# 证书轮换和主动重连

本文档说明 OxiProxy 的证书轮换机制和 Client 主动重连流程。

## 架构概述

```
┌─────────────────────────────────────────────────────────────┐
│                    证书更新完整流程                          │
├─────────────────────────────────────────────────────────────┤
│                                                             │
│  1. Controller 更新证书                                      │
│     └─> CertificateManager::force_update_node_certificate() │
│                                                             │
│  2. Controller 推送证书给 Node                               │
│     └─> NodeManager::push_certificate_update()             │
│                                                             │
│  3. Node 更新证书（热更新）                                  │
│     └─> ProxyServer::update_certificate()                  │
│                                                             │
│  4. Controller 推送新指纹给 Client                           │
│     └─> ClientStreamManager::push_proxy_list_update()      │
│                                                             │
│  5. Client 检测指纹变化                                      │
│     └─> ConnectionManager::reconcile()                     │
│         ├─> 检测 cert_fingerprints 变化                     │
│         └─> 触发重连                                        │
│                                                             │
│  6. Client 断开旧连接                                        │
│     └─> cancel_token.cancel()                              │
│                                                             │
│  7. Client 使用新指纹建立新连接                              │
│     └─> QuicConnector::new(Fingerprint(new_fingerprints))  │
│                                                             │
└─────────────────────────────────────────────────────────────┘
```

## 核心组件

### 1. Controller 端

#### CertificateManager

```rust
// 强制更新节点证书（用于证书轮换）
pub async fn force_update_node_certificate(
    node_id: i64,
    node_name: &str,
) -> Result<NodeCertificateData>
```

- 总是生成新证书（不检查过期时间）
- 更新数据库中的证书记录
- 返回新证书数据（包含新指纹）

#### NodeManager

```rust
// 向节点推送证书更新
pub async fn push_certificate_update(
    &self,
    node_id: i64,
    certificate: oxiproxy::NodeCertificate,
) -> Result<()>
```

- 通过 gRPC 发送 UpdateCertificateCommand
- 等待 Node 响应确认
- 返回更新结果

### 2. Node 端

#### ProxyServer

```rust
// 更新证书（热更新，无需重启）
pub async fn update_certificate(
    &self,
    cert_pem: String,
    key_pem: String,
) -> Result<()>
```

- 解析新证书 PEM 格式
- 使用 RwLock 更新内存中的证书
- 新连接自动使用新证书
- 现有连接继续使用旧证书

### 3. Client 端

#### ConnectionManager

```rust
// 根据新的代理分组列表，调和连接状态
pub async fn reconcile(&self, server_groups: Vec<ServerProxyGroup>)
```

- 检测 `cert_fingerprints` 是否变化
- 如果变化，断开旧连接并重连
- 使用新指纹创建 QuicConnector
- 验证 Node 证书

## 使用场景

### 场景 1：定期证书轮换

```rust
// 每月轮换一次证书
async fn rotate_certificates_monthly() {
    let nodes = get_all_nodes().await;

    for node in nodes {
        // 1. 生成新证书
        let cert_data = CertificateManager::force_update_node_certificate(
            node.id,
            &node.name,
        ).await?;

        // 2. 推送给 Node
        let certificate = oxiproxy::NodeCertificate {
            cert_pem: cert_data.cert_pem,
            key_pem: cert_data.key_pem,
            fingerprint: cert_data.fingerprint,
            expires_at: cert_data.expires_at,
        };

        node_manager.push_certificate_update(node.id, certificate).await?;

        // 3. 推送新指纹给所有使用该 Node 的 Client
        client_stream_manager.notify_clients_for_node(node.id).await?;
    }
}
```

### 场景 2：证书泄露应急响应

```rust
// 紧急更换泄露的证书
async fn emergency_certificate_rotation(node_id: i64) {
    // 1. 立即生成新证书
    let cert_data = CertificateManager::force_update_node_certificate(
        node_id,
        &get_node_name(node_id),
    ).await?;

    // 2. 推送给 Node（热更新）
    node_manager.push_certificate_update(node_id, cert_data.into()).await?;

    // 3. 强制所有 Client 重连
    client_stream_manager.force_reconnect_for_node(node_id).await?;
}
```

### 场景 3：证书即将过期自动续期

```rust
// 检查证书过期时间并自动续期
async fn auto_renew_expiring_certificates() {
    let expiring_nodes = find_nodes_with_expiring_certificates(30).await; // 30天内过期

    for node in expiring_nodes {
        info!("证书即将过期，自动续期: 节点 #{}", node.id);

        // 自动续期流程
        let cert_data = CertificateManager::force_update_node_certificate(
            node.id,
            &node.name,
        ).await?;

        node_manager.push_certificate_update(node.id, cert_data.into()).await?;
        client_stream_manager.notify_clients_for_node(node.id).await?;
    }
}
```

## 重连行为

### Client 重连触发条件

1. **证书指纹变化**：`cert_fingerprints` 不同
2. **连接任务终止**：`handle.is_finished()` 为 true
3. **节点不再需要**：节点从配置中移除

### 重连流程

```
1. 检测到指纹变化
   ↓
2. 记录日志（旧指纹 → 新指纹）
   ↓
3. 取消旧连接（cancel_token.cancel()）
   ↓
4. 等待旧连接任务结束
   ↓
5. 创建新 QuicConnector（使用新指纹）
   ↓
6. 建立新 QUIC 连接
   ↓
7. 验证 Node 证书指纹
   ↓
8. 连接成功 ✅
```

### 重连日志示例

```
[INFO] 🔄 节点 #1 证书指纹已更新，触发重连以使用新证书
[INFO]    旧指纹: ["sha256:abc123..."]
[INFO]    新指纹: ["sha256:def456..."]
[INFO] 🔌 断开节点 #1 的旧连接
[INFO] 节点 #1 连接已取消
[INFO] 连接到节点 #1 (192.168.1.100:7000), 协议: Quic, 代理数: 3
[INFO] 节点 #1 使用证书指纹验证（1 个指纹）
[DEBUG] ✅ 证书指纹验证通过: sha256:def456...
[INFO] 📡 新连接来自: 192.168.1.100:7000
```

## 安全考虑

### 1. 证书验证

- Client 使用 SHA-256 指纹验证 Node 证书
- 指纹不匹配时拒绝连接
- 防止中间人攻击

### 2. 平滑过渡

- 现有连接继续使用旧证书
- 新连接使用新证书
- 避免服务中断

### 3. 审计日志

- 记录所有证书更新操作
- 记录重连触发原因
- 便于安全审计

## 性能影响

### 证书更新

- **Node 端**：热更新，无需重启，影响极小
- **数据库**：单条 UPDATE 操作
- **gRPC**：单次命令推送

### Client 重连

- **断开时间**：< 1 秒
- **重连时间**：1-5 秒（取决于网络）
- **流量影响**：短暂中断，自动恢复

### 批量更新

- 建议分批更新节点（每批 10-20 个）
- 避免同时更新所有节点
- 降低对 Client 的影响

## 故障排查

### 问题 1：Client 未重连

**症状**：证书更新后 Client 仍使用旧证书

**排查步骤**：
1. 检查 Controller 是否推送了 ProxyListUpdate
2. 检查 Client 日志是否收到更新
3. 检查指纹是否真的变化了

### 问题 2：重连失败

**症状**：Client 重连后无法建立连接

**排查步骤**：
1. 检查 Node 是否成功更新证书
2. 检查新指纹是否正确
3. 检查网络连接是否正常

### 问题 3：证书验证失败

**症状**：Client 报告证书指纹验证失败

**排查步骤**：
1. 检查 Controller 数据库中的指纹
2. 检查 Node 实际使用的证书
3. 检查指纹计算是否一致

## 最佳实践

1. **定期轮换**：每 3-6 个月轮换一次证书
2. **低峰期操作**：在流量低峰期进行证书更新
3. **分批更新**：避免同时更新所有节点
4. **监控告警**：监控证书更新成功率
5. **备份证书**：保留旧证书备份以便回滚
6. **测试验证**：在测试环境先验证流程

## API 参考

### Controller API

```rust
// 强制更新节点证书
POST /api/admin/nodes/{node_id}/rotate-certificate

// 批量更新证书
POST /api/admin/certificates/rotate-all

// 查询证书状态
GET /api/admin/nodes/{node_id}/certificate
```

### gRPC 命令

```protobuf
// 更新证书命令
message UpdateCertificateCommand {
  string request_id = 1;
  NodeCertificate certificate = 2;
}

// 更新证书响应
message UpdateCertificateResponse {
  bool success = 1;
  optional string error = 2;
}
```

## 总结

OxiProxy 的证书轮换和主动重连机制提供了：

- ✅ **零停机更新**：Node 热更新证书
- ✅ **自动重连**：Client 检测指纹变化自动重连
- ✅ **安全验证**：SHA-256 指纹验证防止中间人攻击
- ✅ **平滑过渡**：现有连接不受影响
- ✅ **完整审计**：所有操作都有日志记录

这使得证书管理完全自动化，大大降低了运维成本和安全风险。
