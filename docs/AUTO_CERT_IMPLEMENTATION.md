# 自动化证书管理实现指南

## 已完成的文件

### 1. 数据库迁移
- ✅ `controller/src/migration/m20260315_000001_add_node_certificates.rs`
  - 创建 `node_certificates` 表
  - 存储每个节点的证书、私钥和指纹

### 2. 数据库实体
- ✅ `controller/src/entity/node_certificate.rs`
  - NodeCertificate 实体定义

### 3. 证书生成工具
- ✅ `controller/src/cert_generator.rs`
  - `generate_self_signed_certificate()` - 生成自签名证书
  - `generate_node_certificate()` - 为节点生成证书

### 4. 证书管理器
- ✅ `controller/src/certificate_manager.rs`
  - `get_or_generate_node_certificate()` - 获取或自动生成证书
  - `get_node_fingerprint()` - 获取证书指纹
  - 自动检查证书过期并续期

---

## 需要修改的文件

### 1. Controller - main.rs

```rust
// controller/src/main.rs

mod cert_generator;
mod certificate_manager;

// 在 entity.rs 中添加
pub use node_certificate::Entity as NodeCertificate;
```

### 2. Controller - gRPC 服务

```rust
// controller/src/grpc_agent_server_service.rs

use crate::certificate_manager::CertificateManager;

// 在 Node 注册成功后，自动生成并返回证书
async fn handle_node_register(&self, node_id: i64, node_name: &str) -> Result<oxiproxy::NodeCertificate> {
    // 获取或生成证书
    let cert_data = CertificateManager::get_or_generate_node_certificate(node_id, &node_name).await?;

    Ok(oxiproxy::NodeCertificate {
        cert_pem: cert_data.cert_pem,
        key_pem: cert_data.key_pem,
        fingerprint: cert_data.fingerprint,
        expires_at: cert_data.expires_at,
    })
}

// 在认证成功后返回证书
let certificate = match self.handle_node_register(node_id, &node_name).await {
    Ok(cert) => Some(cert),
    Err(e) => {
        tracing::error!("生成节点证书失败: {}", e);
        None
    }
};

// 发送注册响应（包含证书）
let register_response = oxiproxy::AgentServerResponse {
    request_id: register_req.request_id.clone(),
    result: Some(oxiproxy::agent_server_response::Result::RegisterAck(
        oxiproxy::RegisterAck {
            success: true,
            error: String::new(),
            node_id,
            certificate,  // 包含证书
        }
    )),
};
```

### 3. Controller - Client 配置下发

```rust
// controller/src/client_stream_manager.rs

use crate::certificate_manager::CertificateManager;

impl ClientStreamManager {
    pub async fn build_proxy_list_update(&self, client_id: i64) -> Result<ProxyListUpdate> {
        // ... 现有逻辑 ...

        // 为每个节点组添加证书指纹
        for group in &mut server_groups {
            if let Some(fingerprint) = CertificateManager::get_node_fingerprint(group.node_id).await? {
                group.cert_fingerprints = vec![fingerprint];
                tracing::debug!("为节点 #{} 添加证书指纹", group.node_id);
            }
        }

        Ok(ProxyListUpdate {
            client_id,
            client_name,
            server_groups,
        })
    }
}
```

### 4. gRPC 协议定义

```protobuf
// common/proto/oxiproxy.proto

message RegisterAck {
    bool success = 1;
    string error = 2;
    int64 node_id = 3;
    optional NodeCertificate certificate = 4;  // 新增
}

message NodeCertificate {
    string cert_pem = 1;
    string key_pem = 2;
    string fingerprint = 3;
    int64 expires_at = 4;
}

message ServerProxyGroup {
    int64 node_id = 1;
    string server_addr = 2;
    uint32 server_port = 3;
    string protocol = 4;
    optional GrpcKcpConfig kcp = 5;
    repeated ProxyInfo proxies = 6;
    repeated string cert_fingerprints = 7;  // 新增
}
```

### 5. Node - 使用证书启动

```rust
// node/src/server/grpc_client.rs

pub async fn connect_and_register(
    controller_url: &str,
    token: &str,
) -> Result<(i64, Option<NodeCertificate>)> {
    let mut client = AgentServerClient::connect(controller_url).await?;

    // 发送注册请求
    let (tx, rx) = mpsc::channel(256);
    let register_msg = AgentServerMessage {
        payload: Some(Payload::Register(NodeRegisterRequest {
            token: token.to_string(),
            request_id: uuid::Uuid::new_v4().to_string(),
        })),
    };

    tx.send(register_msg).await?;

    let mut stream = client.agent_server_channel(ReceiverStream::new(rx)).await?.into_inner();

    // 接收注册响应
    if let Some(msg) = stream.next().await {
        let response = msg?;
        if let Some(Result::RegisterAck(ack)) = response.result {
            if ack.success {
                return Ok((ack.node_id, ack.certificate));
            }
        }
    }

    Err(anyhow!("注册失败"))
}

// node/src/main.rs

#[tokio::main]
async fn main() -> Result<()> {
    // 连接 Controller 并获取证书
    let (node_id, certificate) = connect_and_register(&controller_url, &token).await?;

    if let Some(cert) = certificate {
        tracing::info!("✅ 从 Controller 获取证书");
        tracing::info!("   指纹: {}", cert.fingerprint);

        // 解析证书
        let cert_der = parse_pem_to_der(&cert.cert_pem)?;
        let key_der = parse_pem_key_to_der(&cert.key_pem)?;

        // 使用证书启动 QUIC 服务
        let listener = QuicListener::new(
            bind_addr,
            cert_der,
            key_der,
            60,
            100,
            5,
        )?;

        start_proxy_server(listener).await?;
    } else {
        return Err(anyhow!("Controller 未返回证书"));
    }

    Ok(())
}
```

### 6. Client - 使用指纹验证

```rust
// client/src/client/connection_manager.rs

impl ConnectionManager {
    async fn connect(&self, group: ServerProxyGroup, proxy_ids: HashSet<i64>) {
        // 从 Controller 下发的配置中获取证书指纹
        let verification_mode = if !group.cert_fingerprints.is_empty() {
            tracing::info!("✅ 使用 Controller 下发的证书指纹验证");
            let fingerprints: HashSet<String> = group.cert_fingerprints.into_iter().collect();
            CertVerificationMode::Fingerprint(fingerprints)
        } else {
            tracing::warn!("⚠️  未配置证书指纹，使用系统 CA 验证");
            CertVerificationMode::SystemCA
        };

        // 创建 QUIC 连接器
        let connector = match group.protocol {
            TunnelProtocol::Quic => {
                QuicConnector::new(verification_mode)?
            }
            // ...
        };

        // 建立连接
        // ...
    }
}
```

---

## 依赖添加

### Controller Cargo.toml

```toml
[dependencies]
rcgen = "0.12"  # 证书生成
base64 = "0.21"  # Base64 编码
```

---

## 工作流程

### 1. Node 启动流程

```
Node 启动
    ↓
连接 Controller gRPC
    ↓
发送 NodeRegisterRequest (token)
    ↓
Controller 验证 token
    ↓
Controller 检查数据库：节点是否有证书？
    ├─ 有且未过期 → 返回现有证书
    └─ 无或已过期 → 自动生成新证书 → 保存到数据库 → 返回证书
    ↓
Node 收到 RegisterAck (包含证书)
    ↓
Node 使用证书启动 QUIC 服务
    ↓
✅ Node 就绪
```

### 2. Client 连接流程

```
Client 启动
    ↓
连接 Controller gRPC
    ↓
Controller 推送 ProxyListUpdate
    ├─ server_groups[0].cert_fingerprints = ["sha256:abc..."]
    └─ server_groups[1].cert_fingerprints = ["sha256:def..."]
    ↓
Client 收到配置
    ↓
Client 创建 QuicConnector (指纹验证模式)
    ↓
Client 连接 Node
    ↓
验证证书指纹是否匹配
    ├─ ✅ 匹配 → 建立连接
    └─ ❌ 不匹配 → 拒绝连接
```

---

## 测试步骤

### 1. 编译

```bash
cargo build --release
```

### 2. 启动 Controller

```bash
./target/release/controller start
```

查看日志应该看到：
```
📋 controller 启动
🌐 Web管理端口: 3000
🔗 内部API端口: 3100
```

### 3. 启动 Node

```bash
./target/release/node --controller-url http://localhost:3100 --token YOUR_NODE_TOKEN
```

查看日志应该看到：
```
✅ 从 Controller 获取证书
   指纹: sha256:a1b2c3d4...
🚀 QUIC 服务已启动: 0.0.0.0:7000
```

### 4. 启动 Client

```bash
./target/release/client --controller-url http://localhost:3100 --token YOUR_CLIENT_TOKEN
```

查看日志应该看到：
```
✅ 使用 Controller 下发的证书指纹验证
🔒 QUIC 使用证书指纹验证（已配置 1 个指纹）
✅ 证书指纹验证通过: sha256:a1b2c3d4...
```

---

## 优势

1. ✅ **零配置**：Node 和 Client 无需手动配置证书
2. ✅ **自动生成**：Controller 自动为每个节点生成证书
3. ✅ **自动下发**：证书和指纹通过 gRPC 自动下发
4. ✅ **自动续期**：证书过期前 30 天自动重新生成
5. ✅ **集中管理**：所有证书在 Controller 数据库统一管理
6. ✅ **安全**：使用证书指纹验证，防止中间人攻击

---

## 故障排查

### 问题 1：Node 未收到证书

**检查**：
```bash
# 查看 Controller 日志
grep "生成节点证书" /var/log/oxiproxy/controller.log

# 查看数据库
sqlite3 data/oxiproxy.db "SELECT * FROM node_certificates;"
```

### 问题 2：Client 指纹验证失败

**检查**：
```bash
# 查看 Client 日志
grep "证书指纹" /var/log/oxiproxy/client.log

# 对比 Node 和 Client 的指纹是否一致
```

### 问题 3：证书过期

**解决**：
- Controller 会自动检测并重新生成
- 或手动删除数据库中的证书记录，重启 Node

---

## 下一步

1. 实现 Web 界面查看证书状态
2. 添加手动刷新证书的 API
3. 添加证书过期告警
4. 支持上传自定义证书（可选）
