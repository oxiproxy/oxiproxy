# QUIC 证书验证配置指南

## 概述

OxiProxy 现在支持三种 QUIC 证书验证模式：

1. **证书指纹验证**（推荐）- 适合自签名证书
2. **系统 CA 验证** - 适合公网 CA 签发的证书
3. **跳过验证**（不安全）- 仅用于开发环境

## 方案 1：证书指纹验证（推荐）

### 适用场景
- 使用自签名证书
- 节点没有公网域名
- 需要高安全性但不想购买 CA 证书

### 配置步骤

#### 1. 在 Node 服务器上生成自签名证书

```bash
# 生成私钥和证书（有效期 10 年）
openssl req -x509 -newkey rsa:4096 -keyout node_key.pem -out node_cert.pem \
    -days 3650 -nodes -subj "/CN=oxiproxy-node"
```

#### 2. 计算证书指纹

**Linux/macOS:**
```bash
./scripts/calculate_cert_fingerprint.sh node_cert.pem
```

**Windows PowerShell:**
```powershell
.\scripts\calculate_cert_fingerprint.ps1 node_cert.pem
```

输出示例：
```
sha256:a1b2c3d4e5f6789012345678901234567890abcdefabcdefabcdefabcdefabcd

✅ 请将此指纹添加到 Client 配置中
```

#### 3. 配置 Node（使用证书）

```bash
# 启动 Node 时指定证书
node --controller-url http://controller:3100 \
     --token <node-token> \
     --cert node_cert.pem \
     --key node_key.pem
```

或在配置文件中：
```toml
# node.toml
tunnel_protocol = "quic"
cert_path = "/path/to/node_cert.pem"
key_path = "/path/to/node_key.pem"
```

#### 4. 配置 Client（使用指纹验证）

```bash
# 启动 Client 时指定证书指纹
client --controller-url http://controller:3100 \
       --token <client-token> \
       --cert-fingerprint sha256:a1b2c3d4e5f6789012345678901234567890abcdefabcdefabcdefabcdefabcd
```

或在配置文件中：
```toml
# client.toml
[quic]
verification_mode = "fingerprint"
allowed_fingerprints = [
    "sha256:a1b2c3d4e5f6789012345678901234567890abcdefabcdefabcdefabcdefabcd"
]
```

#### 5. 通过 Controller 管理（推荐）

在 Controller 的 Web 界面中：
1. 进入"节点管理"
2. 编辑节点配置
3. 在"证书指纹"字段中粘贴指纹
4. Controller 会自动将指纹下发给 Client

---

## 方案 2：系统 CA 验证

### 适用场景
- 使用 Let's Encrypt 等公网 CA 签发的证书
- 节点有公网域名
- 需要标准的 TLS 验证

### 配置步骤

#### 1. 获取 CA 签发的证书

使用 Let's Encrypt（免费）：
```bash
# 安装 certbot
sudo apt install certbot

# 获取证书（需要域名和 80/443 端口）
sudo certbot certonly --standalone -d node.yourdomain.com
```

证书位置：
- 证书：`/etc/letsencrypt/live/node.yourdomain.com/fullchain.pem`
- 私钥：`/etc/letsencrypt/live/node.yourdomain.com/privkey.pem`

#### 2. 配置 Node

```bash
node --controller-url http://controller:3100 \
     --token <node-token> \
     --cert /etc/letsencrypt/live/node.yourdomain.com/fullchain.pem \
     --key /etc/letsencrypt/live/node.yourdomain.com/privkey.pem
```

#### 3. 配置 Client

```bash
# 使用系统 CA 验证（默认）
client --controller-url http://controller:3100 \
       --token <client-token> \
       --cert-verification system-ca
```

或在配置文件中：
```toml
# client.toml
[quic]
verification_mode = "system_ca"
```

---

## 方案 3：跳过验证（仅开发环境）

### ⚠️ 警告
此模式会接受任何证书，包括伪造的证书。**仅用于开发和测试环境，生产环境禁止使用！**

### 配置

```bash
# Client 启动时跳过验证
client --controller-url http://controller:3100 \
       --token <client-token> \
       --cert-verification skip

# 或设置环境变量
export OXIPROXY_INSECURE_QUIC=1
client --controller-url http://controller:3100 --token <client-token>
```

---

## 代码示例

### Rust 代码中使用

```rust
use common::tunnel::quic::{QuicConnector, CertVerificationMode};
use std::collections::HashSet;

// 方案 1：证书指纹验证
let fingerprints = HashSet::from([
    "sha256:a1b2c3d4...".to_string(),
    "sha256:b2c3d4e5...".to_string(),  // 可以配置多个指纹
]);
let connector = QuicConnector::new(
    CertVerificationMode::Fingerprint(fingerprints)
)?;

// 方案 2：系统 CA 验证
let connector = QuicConnector::new(
    CertVerificationMode::SystemCA
)?;

// 方案 3：跳过验证（不推荐）
let connector = QuicConnector::new(
    CertVerificationMode::SkipVerification
)?;
```

---

## 安全最佳实践

### 1. 证书轮换

定期更新证书（建议每年一次）：
```bash
# 生成新证书
openssl req -x509 -newkey rsa:4096 -keyout node_key_new.pem \
    -out node_cert_new.pem -days 3650 -nodes -subj "/CN=oxiproxy-node"

# 计算新指纹
./scripts/calculate_cert_fingerprint.sh node_cert_new.pem

# 在 Controller 中添加新指纹（保留旧指纹）
# 等待所有 Client 更新配置后，再移除旧指纹
```

### 2. 指纹管理

- 在 Controller 数据库中集中管理指纹
- 支持多个指纹（用于证书轮换期间）
- 定期审计指纹列表

### 3. 监控告警

```rust
// 在日志中记录证书验证失败
tracing::error!("❌ 证书指纹验证失败: {}", fingerprint);

// 设置告警规则
// 如果短时间内大量证书验证失败，可能是中间人攻击
```

---

## 故障排查

### 问题 1：证书指纹不匹配

**错误信息：**
```
❌ 证书指纹验证失败: sha256:xxx
允许的指纹: ["sha256:yyy"]
```

**解决方法：**
1. 重新计算证书指纹：`./scripts/calculate_cert_fingerprint.sh node_cert.pem`
2. 确认 Node 使用的证书文件正确
3. 检查 Client 配置中的指纹是否正确

### 问题 2：系统 CA 验证失败

**错误信息：**
```
certificate verify failed: unable to get local issuer certificate
```

**解决方法：**
1. 确认证书是完整链（包含中间证书）
2. 更新系统 CA 证书：`sudo update-ca-certificates`
3. 检查证书是否过期：`openssl x509 -in cert.pem -noout -dates`

### 问题 3：连接超时

**可能原因：**
- 防火墙阻止 UDP 流量（QUIC 使用 UDP）
- 证书验证失败导致握手中断

**解决方法：**
```bash
# 检查 UDP 端口是否开放
sudo ufw allow 7000/udp

# 查看详细日志
RUST_LOG=debug client --controller-url ...
```

---

## 性能影响

证书验证对性能的影响：

| 验证模式 | 首次连接延迟 | CPU 开销 | 安全性 |
|---------|------------|---------|--------|
| 跳过验证 | ~5ms | 极低 | ❌ 不安全 |
| 指纹验证 | ~10ms | 低 | ✅ 高 |
| 系统 CA | ~15ms | 中 | ✅ 最高 |

**建议**：对于内网穿透场景，证书指纹验证是性能和安全性的最佳平衡点。

---

## 迁移指南

### 从"跳过验证"迁移到"指纹验证"

1. **生成证书**（如果还没有）
   ```bash
   openssl req -x509 -newkey rsa:4096 -keyout node_key.pem \
       -out node_cert.pem -days 3650 -nodes -subj "/CN=oxiproxy-node"
   ```

2. **计算指纹**
   ```bash
   ./scripts/calculate_cert_fingerprint.sh node_cert.pem
   ```

3. **更新 Node 配置**（添加证书路径）

4. **更新 Client 配置**（添加指纹验证）

5. **重启服务**（先 Node，后 Client）

6. **验证连接**
   ```bash
   # 查看日志确认使用指纹验证
   grep "证书指纹验证" /var/log/oxiproxy/client.log
   ```

---

## 常见问题

**Q: 可以使用多个证书指纹吗？**
A: 可以。这对于证书轮换很有用，可以同时配置新旧证书的指纹。

**Q: 指纹验证和域名验证有什么区别？**
A: 指纹验证直接验证证书内容，不需要域名。域名验证（系统 CA）需要证书中的域名与连接地址匹配。

**Q: 如何在 Docker 中使用？**
A: 将证书文件挂载到容器中：
```bash
docker run -v /path/to/certs:/certs oxiproxy/node \
    --cert /certs/node_cert.pem --key /certs/node_key.pem
```

**Q: 证书过期了怎么办？**
A: 重新生成证书，计算新指纹，更新配置。建议在证书过期前 30 天开始轮换。
