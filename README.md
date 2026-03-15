<div align="center">

# OxiProxy

**基于 Rust 的高性能内网穿透工具**

[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](https://opensource.org/licenses/MIT)
[![Rust](https://img.shields.io/badge/Rust-2021_Edition-orange.svg)](https://www.rust-lang.org/)
[![QUIC](https://img.shields.io/badge/Protocol-QUIC%2FKCP%2FTCP-blue.svg)](https://quicwg.org/)

一个现代化的内网穿透解决方案，采用 Rust + QUIC/KCP/TCP + React 技术栈，提供三层架构的高性能反向代理服务。

[快速开始](#-快速开始) | [安装教程](#-安装教程) | [配置说明](#-配置说明) | [Web 管理界面](#-web-管理界面) | [架构](#-架构)

</div>

## 特性

| 特性 | 说明 |
|------|------|
| **高性能** | 基于 Rust + QUIC/KCP/TCP 协议，低延迟、高并发 |
| **安全可靠** | 自动化证书管理 + TLS 加密传输 + Token/JWT 认证 |
| **三层架构** | Controller + Node + Client 灵活部署 |
| **跨平台** | 支持 Linux、Windows、macOS (amd64/arm64) |
| **易于使用** | 一键安装脚本 + Web 可视化管理界面 |
| **多协议** | 支持 QUIC（默认）、KCP、TCP 三种隧道协议，适配不同网络环境 |
| **证书管理** | 自动生成、分发、验证证书，零配置安全连接 |
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

只需三步，即可完成内网穿透部署。

### 1. 安装 Controller（服务端）

在公网服务器上执行：

```bash
curl -fsSL https://raw.githubusercontent.com/oxiproxy/oxiproxy/master/install.sh | bash
```

选择安装 `controller`，安装完成后启动：

```bash
cd /opt/oxiproxy
./controller
```

首次启动会自动生成 admin 密码，保存在 `data/admin_password.txt`。

### 2. 登录 Web 管理界面

打开 `http://your-server-ip:3000`，使用 admin 账号和生成的密码登录。

在管理界面中完成以下操作：
1. **节点管理** → 创建节点 → 复制节点 Token
2. 在同一台服务器上安装并启动 Node（见下方）
3. **客户端管理** → 创建客户端 → 复制客户端 Token
4. **隧道管理** → 创建隧道 → 配置端口映射

### 3. 安装 Node 和 Client

**安装 Node（节点服务器，与 Controller 可部署在同一台或不同服务器）：**

```bash
curl -fsSL https://raw.githubusercontent.com/oxiproxy/oxiproxy/master/install.sh | bash
# 选择安装 node

/opt/oxiproxy/node start --controller-url http://your-server-ip:3100 --token <node-token>
```

**安装 Client（内网机器上）：**

```bash
curl -fsSL https://raw.githubusercontent.com/oxiproxy/oxiproxy/master/install.sh | bash
# 选择安装 client

/opt/oxiproxy/client start --controller-url http://your-server-ip:3100 --token <client-token>
```

### 使用示例

在 Web 管理界面创建隧道后，即可通过 Node 的公网 IP 访问内网服务：

| 场景 | 本地端口 | 远程端口 | 访问方式 |
|------|---------|---------|----------|
| SSH | 22 | 2222 | `ssh -p 2222 user@node-ip` |
| 远程桌面 | 3389 | 33389 | RDP 连接 `node-ip:33389` |
| Web 服务 | 80 | 8080 | 访问 `http://node-ip:8080` |
| MySQL | 3306 | 13306 | 连接 `node-ip:13306` |

## 安装教程

| 方式 | 适用场景 | 难度 |
|------|---------|------|
| [一键安装脚本](#一键安装脚本推荐) | Linux/macOS 快速部署，推荐 | ⭐ |
| [Docker Compose](#docker-compose-安装) | 容器化部署 | ⭐⭐ |
| [Docker](#docker-安装) | 单容器部署 | ⭐⭐ |
| [原生安装](#原生安装) | 手动下载或源码编译 | ⭐⭐⭐ |

### 一键安装脚本（推荐）

适用于 Linux 和 macOS，支持镜像加速（国内服务器友好）：

```bash
curl -fsSL https://raw.githubusercontent.com/oxiproxy/oxiproxy/master/install.sh | bash
```

脚本支持：

- **交互式选择** 安装组件（controller / node / client / 全部）
- **自动检测架构**（x86_64 / ARM64）
- **多镜像源** 自动切换（GitHub、ghfast.top、gh-proxy.com、ghproxy.cc）
- **指定版本** 和安装目录
- **更新** 已安装的组件

安装完成后启动服务：

```bash
# Controller
/opt/oxiproxy/controller

# Node（隧道端口由 Controller 下发，无需手动指定）
/opt/oxiproxy/node start --controller-url http://controller-ip:3100 --token <node-token>

# Client
/opt/oxiproxy/client start --controller-url http://controller-ip:3100 --token <client-token>
```

<details>
<summary><b>守护进程模式（后台运行）</b></summary>

```bash
# Linux/macOS —— 使用 daemon 子命令
/opt/oxiproxy/node daemon --controller-url http://controller-ip:3100 --token <token>
/opt/oxiproxy/client daemon --controller-url http://controller-ip:3100 --token <token>

# 或配置为 systemd 服务（见下方）
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
ExecStart=/opt/oxiproxy/controller
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
ExecStart=/opt/oxiproxy/node start --controller-url http://controller-ip:3100 --token your-node-token
Restart=always

[Install]
WantedBy=multi-user.target
EOF

sudo systemctl daemon-reload
sudo systemctl enable --now oxiproxy-node
```

**Client 服务：**
```bash
sudo tee /etc/systemd/system/oxiproxy-client.service > /dev/null << EOF
[Unit]
Description=OxiProxy Client
After=network.target

[Service]
Type=simple
ExecStart=/opt/oxiproxy/client start --controller-url http://controller-ip:3100 --token your-client-token
Restart=always

[Install]
WantedBy=multi-user.target
EOF

sudo systemctl daemon-reload
sudo systemctl enable --now oxiproxy-client
```

</details>

<details>
<summary><b>Windows 服务模式</b></summary>

```powershell
# 安装为 Windows 服务（需要管理员权限）
.\client.exe install-service --controller-url http://your-server-ip:3100 --token your-client-token

# 启动/停止服务
sc start OxiProxyClient
sc stop OxiProxyClient

# 卸载服务
.\client.exe uninstall-service
```

</details>

---

### Docker Compose 安装

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
curl -O https://raw.githubusercontent.com/oxiproxy/oxiproxy/master/docker/docker-compose.yml

# 启动服务
mkdir -p data && docker compose up -d

# 获取 admin 初始密码
docker compose logs controller
```

> **重要**: 首次启动后查看日志获取 admin 密码，访问 `http://your-server-ip:3000` 登录。密码也保存在 `data/admin_password.txt` 中。

#### 部署 Client

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
      - start
      - --controller-url
      - http://your-server-ip:3100
      - --token
      - your-client-token
EOF

docker compose up -d
```

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
<summary><b>Controller</b></summary>

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
<summary><b>Node</b></summary>

```bash
docker run -d --name oxiproxy-node --restart unless-stopped \
  -p 7000:7000/udp -p 7000:7000/tcp \
  -e TZ=Asia/Shanghai -e RUST_LOG=info,tokio_kcp=off \
  ghcr.io/oxiproxy/oxiproxy:latest \
  /app/node start --controller-url http://your-controller-ip:3100 --token your-node-token
```

> Node 隧道端口由 Controller 下发，默认使用 QUIC (UDP)。如使用 TCP 协议，需同时放行 TCP 端口。

</details>

<details>
<summary><b>Client</b></summary>

```bash
docker run -d --name oxiproxy-client --restart unless-stopped \
  --network host \
  -e TZ=Asia/Shanghai -e RUST_LOG=info,tokio_kcp=off \
  ghcr.io/oxiproxy/oxiproxy:latest \
  /app/client start --controller-url http://your-controller-ip:3100 --token your-client-token
```

</details>

---

### 原生安装

<details>
<summary><b>预编译二进制文件</b></summary>

从 [Releases](https://github.com/oxiproxy/oxiproxy/releases) 下载对应平台的压缩包：

| 平台 | 下载文件 |
|------|---------|
| Linux x86_64 | `*-x86_64-unknown-linux-gnu.tar.gz` |
| Linux ARM64 | `*-aarch64-unknown-linux-gnu.tar.gz` |
| Windows x86_64 | `*-x86_64-pc-windows-msvc.zip` |
| macOS Intel | `*-x86_64-apple-darwin.tar.gz` |
| macOS Apple Silicon | `*-aarch64-apple-darwin.tar.gz` |

每个组件（controller / node / client）独立打包。

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

## 配置说明

### 端口说明

| 端口 | 协议 | 组件 | 说明 |
|------|------|------|------|
| 3000 | TCP | Controller | Web 管理界面 |
| 3100 | TCP | Controller | gRPC 服务（Node/Client 连接） |
| 7000 | UDP/TCP | Node | QUIC/KCP (UDP) 或 TCP 隧道服务（端口由 Controller 下发） |

<details>
<summary><b>配置防火墙</b></summary>

```bash
# Ubuntu/Debian (ufw)
sudo ufw allow 3000/tcp   # Web 界面
sudo ufw allow 3100/tcp   # gRPC 服务
sudo ufw allow 7000/udp   # QUIC/KCP 隧道端口
sudo ufw allow 7000/tcp   # TCP 隧道端口（可选）
sudo ufw reload

# CentOS/RHEL (firewalld)
sudo firewall-cmd --permanent --add-port=3000/tcp
sudo firewall-cmd --permanent --add-port=3100/tcp
sudo firewall-cmd --permanent --add-port=7000/udp
sudo firewall-cmd --permanent --add-port=7000/tcp
sudo firewall-cmd --reload
```

</details>

### Controller 配置

配置优先级：环境变量 → 数据库 SystemConfig 表 → TOML 配置文件 → 默认值。

| 环境变量 | 说明 | 默认值 |
|----------|------|--------|
| `JWT_SECRET` | JWT 签名密钥 | 自动生成 |
| `DATABASE_URL` | SQLite 数据库路径 | `data/oxiproxy.db` |
| `RUST_LOG` | 日志级别 | `info` |

### Node 命令行参数

Node 使用子命令模式：`node <子命令> [参数]`

| 子命令 | 说明 |
|--------|------|
| `start` | 前台运行节点服务器 |
| `daemon` | 以守护进程模式运行（含 `--pid-file`、`--log-dir`） |
| `stop` | 停止运行中的守护进程（含 `--pid-file`） |
| `update` | 自动更新到最新版本 |

**`start` / `daemon` 通用参数：**

| 参数 | 说明 | 必需 |
|------|------|------|
| `--controller-url` | Controller gRPC 地址（如 `http://server:3100`） | 是 |
| `--token` | 节点认证令牌 | 是 |
| `--protocol` | 隧道协议（quic/kcp/tcp），默认 quic | 否 |
| `--tls-ca-cert` | 自定义 CA 证书文件路径（PEM 格式） | 否 |
| `--log-dir` | 日志目录路径（按天自动分割） | 否 |

### Client 命令行参数

Client 使用子命令模式：`client <子命令> [参数]`

| 子命令 | 说明 |
|--------|------|
| `start` | 前台运行客户端 |
| `daemon` | 以守护进程模式运行（含 `--pid-file`、`--log-dir`） |
| `stop` | 停止运行中的守护进程（含 `--pid-file`） |
| `install-service` | 安装为 Windows 服务（仅 Windows） |
| `uninstall-service` | 卸载 Windows 服务（仅 Windows） |
| `update` | 自动更新到最新版本 |

**`start` / `daemon` 通用参数：**

| 参数 | 说明 | 必需 |
|------|------|------|
| `--controller-url` | Controller gRPC 地址（如 `http://server:3100`） | 是 |
| `--token` | 客户端认证令牌 | 是 |
| `--tls-ca-cert` | 自定义 CA 证书文件路径（PEM 格式） | 否 |
| `--log-dir` | 日志目录路径（按天自动分割） | 否 |

## Web 管理界面

### 功能模块

| 模块 | 功能 |
|------|------|
| **仪表盘** | 总览统计、流量统计、实时在线状态监控 |
| **节点管理** | 查看在线状态、管理 Token、分配节点给用户 |
| **客户端管理** | 创建/删除客户端、生成 Token、查看流量统计 |
| **隧道管理** | 创建/编辑/删除隧道、支持 TCP/UDP、代理组和子代理 |
| **用户管理** | 创建/编辑/删除用户、分配节点和流量配额（管理员） |
| **订阅套餐** | 创建/编辑套餐、配置节点/客户端/流量配额 |

<details>
<summary><b>API 接口</b></summary>

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
| `/users` | GET/POST | 用户列表/创建（管理员） |
| `/users/{id}` | PUT/DELETE | 用户更新/删除（管理员） |
| `/subscriptions` | GET/POST | 订阅套餐管理（管理员） |

</details>

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
- [rcgen](https://github.com/rustls/rcgen) - 自签名证书生成
- [rustls](https://github.com/rustls/rustls) - TLS 实现（证书指纹验证）
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

## 安全性

- **自动化证书管理**：Controller 自动为每个 Node 生成唯一证书，通过 gRPC 自动分发
- **证书指纹验证**：Client 使用 SHA-256 指纹验证 Node 证书，防止中间人攻击
- **证书自动续期**：证书有效期 10 年，提前 30 天自动更新，零配置管理
- **TLS 加密**：QUIC 协议内置 TLS 加密，隧道通信安全
- **多路复用**：KCP 和 TCP 隧道使用 yamux 多路复用，TCP 隧道适用于 UDP 受限网络
- **Token 认证**：Node 和 Client 使用 Token 进行身份验证
- **JWT 认证**：Web 界面使用 JWT 进行用户认证
- **密码加密**：用户密码使用 bcrypt 加密存储
- **权限校验**：管理员接口和资源归属权校验，防止越权访问

## 故障排除

<details>
<summary><b>Controller 启动失败</b></summary>

- 检查端口 3000 和 3100 是否被占用
- 检查数据库文件权限：`ls -la data/`
- 查看日志：`docker compose logs controller` 或控制台输出

</details>

<details>
<summary><b>Node/Client 无法连接到 Controller</b></summary>

- 确认 Controller gRPC 端口（3100）可访问
- 检查 Token 是否正确
- 检查防火墙是否放行 3100/tcp
- 查看日志排查连接错误

</details>

<details>
<summary><b>忘记 admin 密码</b></summary>

```bash
# 查看首次生成的密码
cat data/admin_password.txt

# 如果文件不存在，可以删除数据库重新生成（会清空所有数据）
rm -f data/oxiproxy.db
# 重启 controller，查看日志获取新密码
```

</details>

<details>
<summary><b>如何更新</b></summary>

```bash
# 脚本安装方式 —— 重新运行安装脚本，选择「更新已安装的组件」
curl -fsSL https://raw.githubusercontent.com/oxiproxy/oxiproxy/master/install.sh | bash

# Docker 方式
docker compose pull && docker compose up -d
```

</details>

<details>
<summary><b>如何备份数据</b></summary>

```bash
tar -czf oxiproxy-backup-$(date +%Y%m%d).tar.gz data/
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

2. **Docker 日志轮转**

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

## 开发

<details>
<summary><b>开发环境搭建</b></summary>

**环境要求**: Rust 1.85+, Bun 1.0+, Protobuf Compiler (protoc)

```bash
git clone https://github.com/oxiproxy/oxiproxy.git
cd oxiproxy

# 构建所有组件
cargo build --release

# 运行 Controller
cargo run --release -p controller

# 运行 Node
cargo run --release -p node -- start --controller-url http://localhost:3100 --token <token>

# 运行 Client
cargo run --release -p client -- start --controller-url http://localhost:3100 --token <token>

# 开发 Dashboard
cd dashboard && bun install && bun run dev
```

</details>

<details>
<summary><b>测试与检查</b></summary>

```bash
cargo test                                             # 运行测试
cargo fmt                                              # 格式化代码
cargo clippy --all-targets --all-features -- -D warnings  # 静态分析
cd dashboard && bun run lint                           # 前端检查
```

</details>

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
