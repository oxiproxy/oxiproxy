# QUIC 证书验证修复 - 快速迁移指南

## 🔴 安全问题

**当前问题**：QUIC 连接跳过所有证书验证，存在中间人攻击风险。

**影响范围**：所有使用 QUIC 协议的 Client-Node 连接。

**修复状态**：✅ 已修复，需要更新配置。

---

## 🚀 快速开始（5 分钟）

### 步骤 1：生成证书（Node 服务器）

```bash
# 在 Node 服务器上执行
openssl req -x509 -newkey rsa:4096 -keyout node_key.pem -out node_cert.pem \
    -days 3650 -nodes -subj "/CN=oxiproxy-node-$(hostname)"

# 设置权限
chmod 600 node_key.pem
chmod 644 node_cert.pem
```

### 步骤 2：计算证书指纹

**Linux/macOS:**
```bash
openssl x509 -in node_cert.pem -noout -fingerprint -sha256 | \
    sed 's/SHA256 Fingerprint=//' | tr -d ':' | tr '[:upper:]' '[:lower:]' | \
    awk '{print "sha256:"$0}'
```

**Windows PowerShell:**
```powershell
$cert = New-Object System.Security.Cryptography.X509Certificates.X509Certificate2("node_cert.pem")
$fp = $cert.GetCertHashString([System.Security.Cryptography.HashAlgorithmName]::SHA256).ToLower()
Write-Host "sha256:$fp"
```

**输出示例：**
```
sha256:a1b2c3d4e5f6789012345678901234567890abcdefabcdefabcdefabcdefabcd
```

### 步骤 3：更新 Node 配置

编辑 Node 启动命令或配置文件：

```bash
# 方式 1：命令行参数
./node --controller-url http://controller:3100 \
       --token YOUR_NODE_TOKEN \
       --cert node_cert.pem \
       --key node_key.pem

# 方式 2：环境变量
export OXIPROXY_NODE_CERT=node_cert.pem
export OXIPROXY_NODE_KEY=node_key.pem
./node --controller-url http://controller:3100 --token YOUR_NODE_TOKEN
```

### 步骤 4：配置 Client（通过 Controller）

**推荐方式**：在 Controller Web 界面配置

1. 登录 Controller 管理界面
2. 进入"节点管理" → 选择节点 → "编辑"
3. 在"证书指纹"字段粘贴步骤 2 的输出
4. 保存配置

Controller 会自动将指纹下发给所有连接到该节点的 Client。

**手动方式**：直接配置 Client

```bash
./client --controller-url http://controller:3100 \
         --token YOUR_CLIENT_TOKEN \
         --cert-fingerprint sha256:a1b2c3d4e5f6...
```

### 步骤 5：重启服务

```bash
# 1. 先重启 Node
systemctl restart oxiproxy-node

# 2. 等待 30 秒后重启 Client
systemctl restart oxiproxy-client

# 3. 检查日志确认连接成功
journalctl -u oxiproxy-client -f | grep "证书指纹验证"
```

---

## ✅ 验证修复

### 检查 Node 日志

```bash
# 应该看到类似输出
grep "QUIC" /var/log/oxiproxy/node.log
```

期望输出：
```
🔒 QUIC 使用证书指纹验证（已配置 1 个指纹）
```

### 检查 Client 日志

```bash
grep "证书指纹" /var/log/oxiproxy/client.log
```

期望输出：
```
✅ 证书指纹验证通过: sha256:a1b2c3d4...
```

### 测试连接

```bash
# 测试隧道是否正常工作
curl http://localhost:YOUR_REMOTE_PORT
```

---

## 🔧 故障排查

### 问题 1：证书指纹不匹配

**症状：**
```
❌ 证书指纹验证失败: sha256:xxx
允许的指纹: ["sha256:yyy"]
```

**解决：**
1. 重新计算 Node 证书指纹（步骤 2）
2. 确认 Client 配置中的指纹与 Node 证书匹配
3. 检查 Node 是否使用了正确的证书文件

### 问题 2：连接超时

**症状：**
```
节点 #1 连接错误: connection timeout
```

**解决：**
1. 检查防火墙是否允许 UDP 流量（QUIC 使用 UDP）
   ```bash
   sudo ufw allow 7000/udp
   ```
2. 检查 Node 是否正常启动
   ```bash
   systemctl status oxiproxy-node
   ```

### 问题 3：证书文件权限错误

**症状：**
```
Error: Permission denied (os error 13)
```

**解决：**
```bash
# 设置正确的权限
chmod 600 node_key.pem
chmod 644 node_cert.pem
chown oxiproxy:oxiproxy node_*.pem
```

---

## 📋 批量部署脚本

### 自动化部署脚本（适用于多节点）

```bash
#!/bin/bash
# deploy_secure_quic.sh

set -e

NODES=("node1.example.com" "node2.example.com" "node3.example.com")
CONTROLLER_URL="http://controller.example.com:3100"

for NODE in "${NODES[@]}"; do
    echo "配置节点: $NODE"

    # 1. 生成证书
    ssh $NODE "cd /opt/oxiproxy && \
        openssl req -x509 -newkey rsa:4096 -keyout node_key.pem -out node_cert.pem \
        -days 3650 -nodes -subj '/CN=oxiproxy-$NODE' && \
        chmod 600 node_key.pem && chmod 644 node_cert.pem"

    # 2. 获取指纹
    FINGERPRINT=$(ssh $NODE "openssl x509 -in /opt/oxiproxy/node_cert.pem -noout -fingerprint -sha256" | \
        sed 's/SHA256 Fingerprint=//' | tr -d ':' | tr '[:upper:]' '[:lower:]' | \
        awk '{print "sha256:"$0}')

    echo "节点 $NODE 指纹: $FINGERPRINT"

    # 3. 更新 Controller 配置（通过 API）
    curl -X PUT "$CONTROLLER_URL/api/nodes/$NODE/fingerprint" \
        -H "Authorization: Bearer $ADMIN_TOKEN" \
        -H "Content-Type: application/json" \
        -d "{\"fingerprint\": \"$FINGERPRINT\"}"

    # 4. 重启 Node
    ssh $NODE "systemctl restart oxiproxy-node"

    echo "✅ 节点 $NODE 配置完成"
    echo ""
done

echo "🎉 所有节点配置完成！"
```

---

## 📚 更多信息

- 完整文档：[docs/QUIC_CERT_VERIFICATION.md](docs/QUIC_CERT_VERIFICATION.md)
- 证书指纹计算工具：
  - Linux/macOS: `scripts/calculate_cert_fingerprint.sh`
  - Windows: `scripts/calculate_cert_fingerprint.ps1`

---

## ⏰ 迁移时间表

| 阶段 | 时间 | 操作 |
|------|------|------|
| 准备 | Day 1 | 生成证书，计算指纹 |
| 测试 | Day 2-3 | 在测试环境验证 |
| 部署 | Day 4-5 | 生产环境逐步部署 |
| 验证 | Day 6-7 | 监控日志，确认无问题 |

---

## 🆘 需要帮助？

如果遇到问题，请：
1. 查看完整文档：`docs/QUIC_CERT_VERIFICATION.md`
2. 检查日志：`journalctl -u oxiproxy-* -f`
3. 提交 Issue：https://github.com/your-repo/oxiproxy/issues
