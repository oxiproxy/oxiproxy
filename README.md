<div align="center">

# OxiProxy

**基于 Rust 的高性能内网穿透工具**

[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](https://opensource.org/licenses/MIT)
[![Rust](https://img.shields.io/badge/Rust-2021_Edition-orange.svg)](https://www.rust-lang.org/)
[![QUIC](https://img.shields.io/badge/Protocol-QUIC%2FKCP%2FTCP-blue.svg)](https://quicwg.org/)

一个现代化的内网穿透解决方案，采用 Rust + QUIC/KCP/TCP + React 技术栈，提供三层架构的高性能反向代理服务。

[特性](#-特性) | [快速开始](#-快速开始) | [安装教程](#-安装教程) | [配置说明](#-配置说明) | [Web 管理界面](#-web-管理界面) | [架构](#-架构)

</div>

## 特性

| 特性 | 说明 |
|------|------|
| **高性能** | 基于 Rust + QUIC/KCP/TCP 协议，低延迟、高并发 |
| **安全可靠** | TLS 加密传输，Token/JWT 认证机制 |
| **三层架构** | Controller + Node + Client 灵活部署 |
| **跨平台** | 支持 Linux、Windows、macOS (amd64/arm64) |
| **易于使用** | Web 可视化管理界面 + 简洁命令行配置 |
| **多协议** | 支持 QUIC（默认）、KCP、TCP 三种隧道协议，适配不同网络环境 |
| **自动重连** | 客户端/节点断线自动重连，服务稳定 |
| **自动更新** | 支持远程自动更新所有组件，运维便捷 |
| **流量管控** | 实时流量统计，支持用户配额管理 |
| **多用户** | 支持多用户、多节点、多客户端、多隧道管理 |
| **订阅套餐** | 支持订阅套餐，灵活分配节点和流量配额 |

<details>
<summary><b>功能详情</b></summary>

**Controller（控制器）**：Web 管理界面、RESTful API、gRPC 服务、SQLite 持久化、JWT 认证、流量统计、用户权限管理、订阅套餐、在线状态监控

**Node（节点服务器）**：QUIC/KCP/TCP 隧道服务、gRPC 连接 Controller、流量记录与批量上报、内存日志缓冲、自动更新

**Client（客户端）**：gRPC 连接 Controller、TCP/UDP 代理、多隧道并发、自动重连、Windows 服务模式、Unix 守护进程模式、自动更新

**Dashboard（Web 界面）**：React 19 + TypeScript + shadcn/ui + Tailwind CSS 4，仪表盘、用户管理、客户端管理、节点管理、隧道管理、流量统计、订阅套餐管理

</details>

## 快速开始

### 1. 部署 Controller + Node

```bash
# Docker Compose 一键部署（推荐）
mkdir -p /opt/oxiproxy && cd /opt/oxiproxy

# 下载 docker-compose.yml
curl -O https://raw.githubusercontent.com/oxiproxy/oxiproxy/master/docker-compose.yml

# 启动 Controller 和 Node
mkdir -p data && docker compose up -d

# 查看日志获取 admin 初始密码
docker compose logs controller
```

### 2. 访问 Web 管理界面

打开 `http://your-server-ip:3000`，使用日志中的密码登录 admin 账号。

### 3. 创建节点、客户端和隧道

1. 进入「节点管理」→ 复制节点 Token → 配置 Node 启动参数
2. 进入「客户端管理」→「新建客户端」→ 复制生成的 Token
3. 进入「隧道管理」→「新建隧道」→ 配置端口映射

### 4. 部署 Client

#### Docker 方式（推荐）

```bash
mkdir -p /opt/oxiproxy-client && cd /opt/oxiproxy-client

cat > docker-compose.yml << 'EOF'
services:
  client:
    image: ghcr.io/oxiproxy/oxiproxy:latest
    container_name: oxiproxy-client
    restart: unless-stopped
    network_mode: host
    environment:
      - TZ=Asia/Shanghai
      - RUST_LOG=info,tokio_kcp=off
    command:
      - /app/client
      - --controller-url
      - http://your-server-ip:3100
      - --token
      - your-client-token
EOF

docker compose up -d
```

#### 原生部署

**Linux/macOS（守护进程模式）**
```bash
# 前台运行
./client --controller-url http://your-server-ip:3100 --token your-client-token

# 守护进程模式
./client --controller-url http://your-server-ip:3100 --token your-client-token --daemon
```

**Windows（服务模式）**
```powershell
# 安装为 Windows 服务（需要管理员权限）
.\client.exe --install-service --controller-url http://your-server-ip:3100 --token your-client-token

# 启动/停止服务
sc start OxiProxyClient
sc stop OxiProxyClient

# 卸载服务
.\client.exe --uninstall-service
```

### 5. 使用示例

| 场景 | 本地端口 | 远程端口 | 访问方式 |
|------|---------|---------|----------|
| SSH | 22 | 2222 | `ssh -p 2222 user@node-ip` |
| 远程桌面 | 3389 | 33389 | RDP 连接 `node-ip:33389` |
| Web 服务 | 80 | 8080 | 访问 `http://node-ip:8080` |
| MySQL | 3306 | 13306 | 连接 `node-ip:13306` |

## 安装教程

OxiProxy 提供四种安装方式：

| 方式 | 适用场景 | 难度 |
|------|---------|------|
| [一键安装脚本](#一键安装脚本) | Linux/macOS 快速部署 | * |
| [Docker Compose](#docker-compose-安装推荐) | 生产环境，推荐 | * |
| [Docker](#docker-安装) | 熟悉 Docker 的用户 | ** |
| [原生安装](#原生安装) | 自定义编译或无 Docker 环境 | *** |

### 一键安装脚本

适用于 Linux 和 macOS，支持镜像加速（国内服务器友好）：

```bash
# 交互式安装（自动选择最快镜像）
curl -fsSL https://raw.githubusercontent.com/oxiproxy/oxiproxy/master/install.sh | bash

# 或下载后手动运行（可自定义参数）
curl -O https://raw.githubusercontent.com/oxiproxy/oxiproxy/master/install.sh
chmod +x install.sh
./install.sh
```

脚本功能：

- 交互式选择安装组件（controller / node / client）
- 自动检测系统架构（x86_64 / ARM64）
- 多个下载镜像源自动切换（GitHub、ghfast.top、gh-proxy.com、ghproxy.cc）
- 支持指定版本号和安装目录

### Docker Compose 安装（推荐）

<details>
<summary><b>前置要求：安装 Docker</b></summary>

**Linux (Ubuntu/Debian/CentOS):**
```bash
curl -fsSL https://get.docker.com | sh
sudo systemctl enable --now docker
sudo usermod -aG docker $USER && newgrp docker
```

**Windows/macOS:** 下载安装 [Docker Desktop](https://www.docker.com/products/docker-desktop/)

</details>

#### 部署 Controller + Node

```bash
mkdir -p /opt/oxiproxy && cd /opt/oxiproxy

# 下载配置文件
curl -O https://raw.githubusercontent.com/oxiproxy/oxiproxy/master/docker-compose.yml

# 启动服务
mkdir -p data && docker compose up -d

# 获取 admin 初始密码
docker compose logs controller
```

> **重要**: 首次启动后查看日志获取 admin 密码，访问 `http://your-server-ip:3000` 登录并修改密码。密码也会保存在 `data/admin_password.txt` 中。

<details>
<summary><b>配置防火墙</b></summary>

```bash
# Ubuntu/Debian (ufw)
sudo ufw allow 3000/tcp   # Web 界面
sudo ufw allow 3100/tcp   # gRPC 服务（Node/Client 连接用）
sudo ufw allow 7000/udp   # QUIC/KCP 隧道端口
sudo ufw allow 7000/tcp   # TCP 隧道端口（可选）
sudo ufw reload

# CentOS/RHEL (firewalld)
sudo firewall-cmd --permanent --add-port=3000/tcp
sudo firewall-cmd --permanent --add-port=3100/tcp
sudo firewall-cmd --permanent --add-port=7000/udp
sudo firewall-cmd --permanent --add-port=7000/tcp  # TCP 隧道（可选）
sudo firewall-cmd --reload
```

</details>

<details>
<summary><b>常用命令</b></summary>

```bash
docker compose up -d                          # 启动
docker compose stop                           # 停止
docker compose restart                        # 重启
docker compose logs -f                        # 查看日志
docker compose pull && docker compose up -d   # 更新
```

</details>

---

### Docker 安装

<details>
<summary><b>Controller 部署</b></summary>

```bash
mkdir -p /opt/oxiproxy/data && cd /opt/oxiproxy

docker run -d --name oxiproxy-controller --restart unless-stopped \
  -p 3000:3000/tcp -p 3100:3100/tcp \
  -v $(pwd)/data:/app/data \
  -e TZ=Asia/Shanghai -e RUST_LOG=info,tokio_kcp=off \
  ghcr.io/oxiproxy/oxiproxy:latest /app/controller

docker logs -f oxiproxy-controller  # 获取 admin 初始密码
```

</details>

<details>
<summary><b>Node 部署</b></summary>

```bash
docker run -d --name oxiproxy-node --restart unless-stopped \
  -p 7000:7000/udp -p 7000:7000/tcp \
  -e TZ=Asia/Shanghai -e RUST_LOG=info,tokio_kcp=off \
  ghcr.io/oxiproxy/oxiproxy:latest \
  /app/node --controller-url http://your-controller-ip:3100 --token your-node-token --bind-port 7000
```

> **提示**: Node 默认使用 QUIC (UDP) 协议，如果客户端使用 TCP 隧道协议，需同时放行 7000/tcp 端口。

</details>

<details>
<summary><b>Client 部署</b></summary>

```bash
docker run -d --name oxiproxy-client --restart unless-stopped \
  --network host \
  -e TZ=Asia/Shanghai -e RUST_LOG=info,tokio_kcp=off \
  ghcr.io/oxiproxy/oxiproxy:latest \
  /app/client --controller-url http://your-controller-ip:3100 --token your-client-token
```

</details>

---

### 原生安装

<details>
<summary><b>预编译二进制文件</b></summary>

从 [Releases](https://github.com/oxiproxy/oxiproxy/releases) 下载对应平台的压缩包，每个包含三个组件：`controller`、`node`、`client`。

| 平台 | 下载文件 |
|------|---------|
| Linux x86_64 | `oxiproxy-vX.Y.Z-x86_64-unknown-linux-gnu.tar.gz` |
| Linux ARM64 | `oxiproxy-vX.Y.Z-aarch64-unknown-linux-gnu.tar.gz` |
| Windows x86_64 | `oxiproxy-vX.Y.Z-x86_64-pc-windows-msvc.zip` |
| macOS Intel | `oxiproxy-vX.Y.Z-x86_64-apple-darwin.tar.gz` |
| macOS Apple Silicon | `oxiproxy-vX.Y.Z-aarch64-apple-darwin.tar.gz` |

```bash
tar -xzf oxiproxy-vX.Y.Z-x86_64-unknown-linux-gnu.tar.gz
cd oxiproxy
chmod +x controller node client
sudo mv controller node client /usr/local/bin/
```

</details>

<details>
<summary><b>从源码编译</b></summary>

**环境要求**: Rust 1.85+, Bun 1.0+, Protobuf Compiler, Git

```bash
git clone https://github.com/oxiproxy/oxiproxy.git && cd oxiproxy

# 构建后端
cargo build --release

# 构建前端
cd dashboard && bun install && bun run build

# 可执行文件位于: target/release/controller, target/release/node, target/release/client
# 前端静态文件输出到: dist/
```

</details>

<details>
<summary><b>配置为 systemd 服务 (Linux)</b></summary>

**Controller 服务：**
```bash
sudo tee /etc/systemd/system/oxiproxy-controller.service > /dev/null << EOF
[Unit]
Description=OxiProxy Controller
After=network.target

[Service]
Type=simple
WorkingDirectory=/opt/oxiproxy
ExecStart=/usr/local/bin/controller
Restart=always

[Install]
WantedBy=multi-user.target
EOF

sudo systemctl daemon-reload
sudo systemctl enable --now oxiproxy-controller
```

**Node 服务：**
```bash
sudo tee /etc/systemd/system/oxiproxy-node.service > /dev/null << EOF
[Unit]
Description=OxiProxy Node
After=network.target

[Service]
Type=simple
ExecStart=/usr/local/bin/node --controller-url http://controller-ip:3100 --token your-node-token --bind-port 7000
Restart=always

[Install]
WantedBy=multi-user.target
EOF

sudo systemctl daemon-reload
sudo systemctl enable --now oxiproxy-node
```

</details>

## 配置说明

### 端口说明

| 端口 | 协议 | 组件 | 说明 |
|------|------|------|------|
| 3000 | TCP | Controller | Web 管理界面 |
| 3100 | TCP | Controller | gRPC 服务（Node/Client 连接） |
| 7000 | UDP/TCP | Node | QUIC/KCP (UDP) 或 TCP 隧道服务 |

### Controller 配置

Controller 配置按以下优先级加载：环境变量 → 数据库 SystemConfig 表 → TOML 配置文件 → 默认值。

| 环境变量 | 说明 | 默认值 |
|----------|------|--------|
| `JWT_SECRET` | JWT 签名密钥 | 自动生成 |
| `DATABASE_URL` | SQLite 数据库路径 | `data/oxiproxy.db` |
| `RUST_LOG` | 日志级别 | `info` |

### Client 命令行参数

| 参数 | 说明 | 必需 |
|------|------|------|
| `--controller-url` | Controller gRPC 地址（如 `http://server:3100`） | 是 |
| `--token` | 客户端认证令牌 | 是 |
| `--daemon` | 守护进程模式（仅 Unix） | 否 |
| `--pid-file` | PID 文件路径（守护进程模式） | 否 |
| `--log-file` | 日志文件路径（守护进程模式） | 否 |
| `--install-service` | 安装为 Windows 服务 | 否 |
| `--uninstall-service` | 卸载 Windows 服务 | 否 |

### Node 命令行参数

| 参数 | 说明 | 必需 |
|------|------|------|
| `--controller-url` | Controller gRPC 地址（如 `http://server:3100`） | 是 |
| `--token` | 节点认证令牌 | 是 |
| `--bind-port` | QUIC/KCP/TCP 监听端口 | 是 |
| `--daemon` | 守护进程模式（仅 Unix） | 否 |

## Web 管理界面

### 功能模块

#### 仪表盘
- 总览统计：用户数、客户端数、节点数、隧道数
- 流量统计：总发送/接收流量
- 实时在线状态监控

#### 节点管理
- 查看节点在线状态
- 管理节点 Token
- 分配节点给用户

#### 客户端管理
- 创建/删除客户端
- 生成客户端 Token
- 查看在线状态和流量统计

#### 隧道管理
- 创建/编辑/删除隧道
- 支持 TCP/UDP 代理类型
- 配置本地和远程端口映射
- 支持代理组和子代理

#### 用户管理（管理员）
- 创建/编辑/删除用户
- 分配节点和流量配额
- 管理用户订阅套餐

#### 订阅套餐管理
- 创建/编辑套餐
- 配置节点数量、客户端数量、流量配额
- 用户订阅和到期自动回退

### API 接口

Controller 提供 RESTful API，前缀为 `/api`：

| 端点 | 方法 | 说明 |
|------|------|------|
| `/auth/login` | POST | 用户登录 |
| `/auth/me` | GET | 获取当前用户信息 |
| `/dashboard/stats/{user_id}` | GET | 仪表盘统计 |
| `/clients` | GET/POST | 客户端列表/创建 |
| `/clients/{id}` | GET/DELETE | 客户端详情/删除 |
| `/proxies` | GET/POST | 隧道列表/创建 |
| `/proxies/{id}` | PUT/DELETE | 隧道更新/删除 |
| `/nodes` | GET/POST | 节点列表/创建 |
| `/nodes/{id}` | PUT/DELETE | 节点更新/删除 |
| `/traffic/overview` | GET | 流量概览 |
| `/users` | GET/POST | 用户列表/创建 |
| `/users/{id}` | PUT/DELETE | 用户更新/删除 |
| `/subscriptions` | GET/POST | 订阅套餐管理 |

## 架构

```
┌──────────────────────────────────────────────────────────────────┐
│                     OxiProxy 三层架构                             │
├──────────────────────────────────────────────────────────────────┤
│                                                                  │
│  Dashboard (React) ──HTTP/REST──> Controller (Axum)              │
│                                        │                         │
│                                        ├──gRPC Stream──> Node    │
│                                        │                  │      │
│                                        │              QUIC/KCP/TCP   │
│                                        │                  │      │
│  本地服务 <──TCP/UDP── Client <──gRPC Stream──┘         公网服务  │
│                                                                  │
│                                 ┌──────────────┐                 │
│                                 │  SQLite DB   │                 │
│                                 └──────────────┘                 │
│                                                                  │
└──────────────────────────────────────────────────────────────────┘
```

### 核心组件

| 组件 | 说明 |
|------|------|
| **Controller** | 中央控制器，提供 Web 管理界面、RESTful API 和 gRPC 服务，管理所有节点和客户端 |
| **Node** | 节点服务器，提供 QUIC/KCP/TCP 隧道服务，通过 gRPC 注册到 Controller |
| **Client** | 客户端，通过 gRPC 连接 Controller 获取配置，建立到 Node 的隧道连接 |
| **Dashboard** | React 19 + TypeScript + shadcn/ui 前端管理界面 |

### 技术栈

**后端：**
- [Rust](https://www.rust-lang.org/) 2021 Edition - 系统编程语言
- [quinn](https://github.com/quinn-rs/quinn) - QUIC 协议实现
- [tokio-kcp](https://github.com/Matrix-Zhang/tokio_kcp) + [yamux](https://github.com/libp2p/rust-yamux) - KCP/TCP 协议 + 多路复用
- [tokio](https://tokio.rs/) - 异步运行时
- [tonic](https://github.com/hyperium/tonic) - gRPC 框架
- [axum](https://github.com/tokio-rs/axum) - Web 框架
- [sea-orm](https://www.sea-ql.org/SeaORM/) - ORM 框架 (SQLite)

**前端：**
- [React 19](https://react.dev/) + [TypeScript](https://www.typescriptlang.org/)
- [shadcn/ui](https://ui.shadcn.com/) + [Radix UI](https://www.radix-ui.com/) - UI 组件
- [Tailwind CSS 4](https://tailwindcss.com/) - 样式框架
- [Lucide](https://lucide.dev/) - 图标库
- [rolldown-vite](https://rolldown.rs/) - 构建工具

## 开发

### 环境要求

- Rust 1.85+
- Bun 1.0+
- Protobuf Compiler (protoc)

### 构建项目

```bash
git clone https://github.com/oxiproxy/oxiproxy.git
cd oxiproxy

# 构建所有组件
cargo build --release

# 运行 Controller
cargo run --release -p controller

# 运行 Node
cargo run --release -p node -- --controller-url http://localhost:3100 --token <token> --bind-port 7000

# 运行 Client
cargo run --release -p client -- --controller-url http://localhost:3100 --token <token>

# 开发 Dashboard
cd dashboard && bun install && bun run dev
```

### 测试与检查

```bash
# 运行测试
cargo test

# 格式化代码
cargo fmt

# 静态分析
cargo clippy --all-targets --all-features -- -D warnings

# 前端检查
cd dashboard && bun run lint
```

## CI/CD

项目使用 GitHub Actions 自动化构建和发布：

- **Docker**: 自动构建 Docker 镜像
- **Release**: 推送 tag 时自动构建多平台二进制文件并创建 Release

```bash
# 创建新版本发布
git tag v1.0.0
git push origin v1.0.0
```

## 安全性

- **TLS 加密**：QUIC 协议内置 TLS 加密，隧道通信安全
- **多路复用**：KCP 和 TCP 隧道使用 yamux 多路复用，TCP 隧道适用于 UDP 受限网络
- **Token 认证**：Node 和 Client 使用 Token 进行身份验证
- **JWT 认证**：Web 界面使用 JWT 进行用户认证
- **密码加密**：用户密码使用 bcrypt 加密存储
- **自签名证书**：QUIC 连接使用 rcgen 自动生成自签名证书

## 故障排除

<details>
<summary><b>Controller 启动失败</b></summary>

- 检查端口 3000 和 3100 是否被占用
- 检查数据库文件权限：`ls -la data/`
- 查看日志：`docker compose logs controller`

</details>

<details>
<summary><b>Node/Client 无法连接到 Controller</b></summary>

- 确认 Controller gRPC 端口（3100）可访问
- 检查 Token 是否正确
- 检查防火墙是否放行 3100/tcp
- 查看日志排查连接错误

</details>

<details>
<summary><b>Windows 服务安装失败</b></summary>

- 确保以管理员权限运行命令提示符或 PowerShell
- 检查是否已存在同名服务：`sc query OxiProxyClient`
- 查看 Windows 事件查看器中的应用程序日志

</details>

<details>
<summary><b>忘记 admin 密码</b></summary>

```bash
# 查看首次生成的密码
cat data/admin_password.txt

# 如果文件不存在，可以删除数据库重新生成（会清空所有数据）
docker compose down
rm -f data/oxiproxy.db
docker compose up -d
docker compose logs controller  # 查看新密码
```

</details>

<details>
<summary><b>如何更新到最新版本</b></summary>

```bash
docker compose pull
docker compose up -d
```

</details>

<details>
<summary><b>如何备份数据</b></summary>

```bash
# 备份数据库
tar -czf oxiproxy-backup-$(date +%Y%m%d).tar.gz data/

# 恢复数据
tar -xzf oxiproxy-backup-YYYYMMDD.tar.gz
```

</details>

## 生产环境建议

1. **使用反向代理**：为 Web 界面配置 Nginx + HTTPS

```nginx
server {
    listen 443 ssl http2;
    server_name oxiproxy.example.com;

    ssl_certificate /path/to/cert.pem;
    ssl_certificate_key /path/to/key.pem;

    location / {
        proxy_pass http://localhost:3000;
        proxy_set_header Host $host;
        proxy_set_header X-Real-IP $remote_addr;
        proxy_set_header X-Forwarded-For $proxy_add_x_forwarded_for;
        proxy_set_header X-Forwarded-Proto $scheme;
    }
}
```

2. **Docker 日志轮转**：防止日志文件过大

```yaml
logging:
  driver: "json-file"
  options:
    max-size: "10m"
    max-file: "3"
```

3. **定期备份数据**

```bash
# 添加到 crontab
0 2 * * * cd /opt/oxiproxy && tar -czf backup/oxiproxy-$(date +\%Y\%m\%d).tar.gz data/
```

## 贡献

欢迎提交 Issue 和 Pull Request！

1. Fork 本仓库
2. 创建特性分支 (`git checkout -b feature/AmazingFeature`)
3. 提交更改 (`git commit -m 'Add some AmazingFeature'`)
4. 推送到分支 (`git push origin feature/AmazingFeature`)
5. 开启 Pull Request

## 许可证

本项目采用 [MIT](LICENSE) 许可证。

## 致谢

- [frp](https://github.com/fatedier/frp) - 灵感来源
- [quinn](https://github.com/quinn-rs/quinn) - QUIC 协议实现
- [Tokio](https://tokio.rs/) - 异步运行时
- [shadcn/ui](https://ui.shadcn.com/) - UI 组件库

---

<div align="center">

**如果这个项目对你有帮助，请给一个 Star！**

</div>
