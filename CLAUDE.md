# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## 项目概述

OxiProxy 是一个基于 Rust 2021 Edition（workspace resolver = "3"）的高性能反向代理工具（内网穿透解决方案），采用三层架构：

- **Controller**：中央控制器，提供 Web 管理界面、RESTful API 和 gRPC 服务
- **Node**：节点服务器，提供 QUIC/KCP 隧道服务，通过 gRPC 连接到 Controller
- **Client**：客户端，通过 gRPC 连接到 Controller，建立到 Node 的隧道连接
- **Dashboard**：React 19 + TypeScript + shadcn/ui + Tailwind CSS 前端管理界面

## 核心架构

### 通信架构

```
Dashboard (React) ──HTTP/REST──> Controller (Axum)
                                      │
                                      ├──gRPC Stream──> Node (节点服务器)
                                      │                      │
                                      │                      └──QUIC/KCP──> 本地服务
                                      │
                                      └──gRPC Stream──> Client (客户端)
                                                             │
                                                             └──TCP/UDP──> 本地服务
```

### 关键概念

- **Node (节点)**：独立的节点服务器程序，提供 QUIC/KCP 隧道服务，通过 gRPC 连接到 Controller
- **Client (客户端)**：独立的客户端程序，通过 gRPC 连接到 Controller，建立到 Node 的隧道连接
- **Proxy (隧道)**：端口映射配置，定义本地端口到远程端口的转发规则
- **User (用户)**：可以被分配多个节点，管理自己的客户端和隧道
- **流量配额**：Admin 分配给 User，User 分配给 Client 的流量限制系统

### Controller 共享状态（AppState）

Controller 的核心共享状态通过 `AppState` 在 Axum handlers 和 gRPC 服务间共享：
- `proxy_control: Arc<dyn ProxyControl>` - 代理控制接口（由 NodeManager 实现）
- `node_manager: Arc<NodeManager>` - 节点 gRPC 流管理
- `client_stream_manager: Arc<ClientStreamManager>` - 客户端 gRPC 流管理
- `auth_provider: Arc<dyn ClientAuthProvider>` - 客户端认证提供者
- `config_manager: Arc<ConfigManager>` - 系统配置管理（DB 存储）
- `config: Arc<Config>` - 运行时配置

### gRPC 双向流

Controller 与 Node/Client 之间使用 gRPC bidirectional streaming：
- **Node**：通过 `AgentServerService` 注册节点并接收代理配置
- **Client**：通过 `AgentClientService` 注册客户端并接收代理配置
- 流管理器：`node_manager.rs` 和 `client_stream_manager.rs` 维护活跃连接
- 请求-响应匹配：`common/src/grpc/pending_requests.rs` 使用 `request_id` UUID 关联

### 隧道协议抽象层

`common/src/tunnel/` 定义了统一的隧道协议抽象：
- `TunnelSendStream` / `TunnelRecvStream` - 统一 I/O 接口
- `TunnelConnection` / `TunnelConnector` / `TunnelListener` - 连接抽象
- 具体实现：`quic.rs`（quinn）、`kcp.rs`（tokio_kcp + yamux 多路复用）

### 配置加载优先级

Controller 配置按以下优先级加载：
1. 环境变量（`JWT_SECRET` 等）
2. 数据库 `SystemConfig` 表
3. TOML 配置文件
4. 默认值（web_port=3000, internal_port=3100, db_path=data/oxiproxy.db）

首次启动时 admin 密码自动生成并保存到 `data/admin_password.txt`。

### 日志收集系统

Node 和 Client 都使用自定义 tracing layer 实现内存日志缓冲（默认 1000 行循环缓冲）。日志不持久化到磁盘，通过 gRPC 和 HTTP API 可查询。

## 常用命令

### 后端开发

```bash
# 构建所有组件
cargo build --release

# 运行 Controller
cargo run --release -p controller

# 运行 Node（节点服务器）
cargo run --release -p node -- --controller-url http://localhost:3100 --token <token>

# 运行 Client（客户端）
cargo run --release -p client -- --controller-url http://localhost:3100 --token <token>

# 运行所有测试
cargo test

# 运行单个 crate 的测试
cargo test -p controller

# 运行指定测试函数
cargo test -p controller -- test_name

# 代码格式化
cargo fmt

# 静态分析
cargo clippy --all-targets --all-features -- -D warnings
```

### 前端开发

```bash
cd dashboard

# 安装依赖
bun install

# 开发模式（热重载）
bun run dev

# 构建生产版本（输出到 ../dist）
bun run build

# 代码检查
bun run lint
```

### 数据库迁移

数据库迁移使用 SeaORM migration，位于 `controller/src/migration/`。迁移在 Controller 启动时自动运行。

创建新迁移：
1. 在 `controller/src/migration/` 创建新文件 `m<YYYYMMDD>_<序号>_<描述>.rs`
2. 实现 `MigrationTrait`
3. 在 `controller/src/migration/mod.rs` 中注册

## 重要文件和模块

### Controller (controller/src/)

- `main.rs` - 启动入口，初始化数据库、gRPC 服务器、Web 服务器、健康监控
- `grpc_server.rs` - gRPC 服务器（端口 3100），注册 AgentServerService 和 AgentClientService
- `grpc_agent_server_service.rs` - Node 的 gRPC 双向流服务
- `grpc_agent_client_service.rs` - Client 的 gRPC 双向流服务
- `node_manager.rs` - 节点管理器，维护 `HashMap<node_id, NodeStream>` 并实现 `ProxyControl` trait
- `client_stream_manager.rs` - 客户端流管理器，维护 `HashMap<client_id, Sender>` 并推送 ProxyListUpdate
- `api/mod.rs` - Axum 路由注册（公开路由、认证路由、管理员路由）
- `api/handlers/` - RESTful API handlers（auth, user, client, proxy, node, traffic, dashboard, subscription, system_config）
- `middleware/auth.rs` - JWT 认证中间件，提取 `AuthUser { id, username, is_admin }`
- `entity/` - SeaORM 数据库实体
- `migration/` - 数据库迁移（28 个迁移文件）
- `traffic.rs` - 流量记录和统计
- `traffic_limiter.rs` - 流量配额验证逻辑
- `port_limiter.rs` - 用户端口范围限制
- `config_manager.rs` - 系统配置管理（DB 键值对存储）
- `config/` - 运行时配置加载（环境变量 → DB → TOML → 默认值）

### Node (node/src/)

- `main.rs` - 启动入口，CLI 参数解析（clap）。支持 `--daemon`（Unix）
- `server/` - 节点服务器实现
  - `proxy_server.rs` - QUIC/KCP 代理服务器
  - `grpc_client.rs` - 连接到 Controller 的 gRPC 客户端（自动重连）
  - `local_proxy_control.rs` - 本地代理控制实现（实现 ProxyControl trait）
  - `traffic.rs` - 流量记录、批量上报
  - `node_logs.rs` - 内存日志缓冲区（自定义 tracing layer）

### Client (client/src/)

- `main.rs` - 启动入口。Unix: 支持 `--daemon`。Windows: 支持 `--install-service` / `--uninstall-service`
- `client/` - 客户端实现
  - `grpc_client.rs` - 连接到 Controller，接收 ProxyListUpdate
  - `connection_manager.rs` - 隧道连接协调（desired vs actual 状态协调）
  - `log_collector.rs` - 内存日志收集（自定义 tracing layer）
- `windows_service.rs` - Windows Service 注册/管理（服务名: OxiProxyClient）

### Common (common/src/)

- `proto/oxiproxy.proto` - gRPC 协议定义（两个 service：AgentServerService, AgentClientService）
- `build.rs` - tonic-build 自动编译 proto 文件（`cargo build` 时自动触发）
- `tunnel/` - 隧道协议抽象层
  - `traits.rs` - 统一的 TunnelSendStream/RecvStream/Connection/Connector/Listener trait
  - `quic.rs` - QUIC 实现（quinn + rcgen 自签名证书）
  - `kcp.rs` - KCP 实现（tokio_kcp + yamux 多路复用）
- `grpc/pending_requests.rs` - request_id 请求-响应匹配工具
- `protocol/` - 共享 trait 定义（ProxyControl, ClientAuthProvider, traffic 等）

### Dashboard (dashboard/src/)

技术栈：React 19 + TypeScript 5.9 + rolldown-vite（别名为 vite）+ shadcn/ui + Radix UI + Tailwind CSS 4 + Lucide 图标 + Babel React Compiler

- `lib/services.ts` - Axios API 客户端
- `lib/types.ts` - TypeScript 类型定义
- `pages/` - React 页面组件（Dashboard, Users, Clients, Nodes, Proxies, Subscriptions）
- `contexts/` - AuthContext（认证状态）、ToastContext（通知）
- `components/` - shadcn/ui 组件 + Layout、ProtectedRoute、ConfirmDialog 等

构建配置（`vite.config.ts`）：使用 manual chunk splitting（vendor-react, vendor-ui），输出到 `../dist`。

## 关键实现细节

### 认证流程

- **Web UI**：用户登录 → `/auth/login` 验证 bcrypt 密码哈希 → 返回 JWT → 后续请求通过 `Authorization: Bearer <token>` 认证
- **Node**：通过 gRPC 流发送 `NodeRegisterRequest`（含 token）→ Controller 验证
- **Client**：通过 gRPC 流发送 `ClientAuthRequest`（含 token）→ Controller 验证
- JWT Secret：优先环境变量 `JWT_SECRET`，其次 DB SystemConfig 表，最后自动生成并保存到 `data/jwt_secret.key`

### 代理配置下发流程

1. Admin 通过 HTTP API 创建/修改 Proxy
2. Controller 存入数据库
3. Controller 通过 ClientStreamManager 向相关 Client 推送 `ProxyListUpdate`
4. Client 的 ConnectionManager 协调 desired vs actual 连接状态
5. Client 建立/关闭到 Node 的 QUIC/KCP 隧道

### 健康监控系统

Controller 启动两个后台任务（每 30 秒）：
- `start_node_health_monitor()` - 检查所有节点的 gRPC 流是否存在，更新 `is_online`
- `start_client_health_monitor()` - 检查所有客户端的 gRPC 流是否存在，更新 `is_online`

### 流量配额系统

三层分配模式：Admin → User（`traffic_quota_gb`）→ Client。`traffic_limiter.rs` 的 `check_user_quota_allocation()` 确保分配不超过可用配额。配额模式优先于传统上传/下载限制。

### 数据库实体关系

- `User` ↔ `UserNode` ↔ `Node` (多对多)
- `User` → `Client` (一对多)
- `Client` → `Proxy` (一对多)
- `Node` → `Proxy` (一对多，可选)
- `User` → `UserSubscription` → `Subscription`

## 开发注意事项

### 添加新的 API 端点

1. 在 `controller/src/api/handlers/` 创建或修改 handler
2. 在 `controller/src/api/mod.rs` 注册路由（注意区分公开/认证/管理员路由组）
3. 更新 `dashboard/src/lib/services.ts` 添加服务方法
4. 更新 `dashboard/src/lib/types.ts` 添加类型定义（如需要）

### 修改数据库 Schema

1. 创建新的 migration 文件
2. 更新对应的 `entity/` 文件
3. 更新 Rust handler 中的请求/响应结构
4. 更新 TypeScript 类型定义
5. 更新前端组件

### gRPC 协议修改

1. 修改 `common/proto/oxiproxy.proto`
2. 运行 `cargo build` 自动重新生成代码（通过 `common/build.rs` 中的 tonic-build）
3. 更新 Controller、Node 和 Client 中的实现

### 添加新的隧道协议

1. 在 `common/src/tunnel/` 中实现 `TunnelSendStream`、`TunnelRecvStream`、`TunnelConnection`、`TunnelListener` 等 trait
2. 在 `common/src/tunnel/protocol.rs` 的 `TunnelProtocol` enum 中添加新变体
3. 更新 Node 的 `proxy_server.rs` 和 Client 的 `connection_manager.rs`

### 平台特定功能

- **Unix**：Node 和 Client 支持 `--daemon` 模式（daemonize crate），含 `--pid-file` 和 `--log-file` 参数
- **Windows**：Client 支持 `--install-service` / `--uninstall-service`（windows-service crate），服务名 `OxiProxyClient`

## 端口配置

默认端口：
- **3000** - Web 管理界面 (HTTP)，Config 中的 `web_port`
- **3100** - Controller gRPC API，Config 中的 `internal_port`
- **7000** - Node 隧道端口 (QUIC/KCP, UDP)

## 环境变量

- `RUST_LOG` - 日志级别（默认：info）
- `DATABASE_URL` - SQLite 数据库路径（默认：data/oxiproxy.db）
- `JWT_SECRET` - JWT 签名密钥（可选，自动生成）

## Docker

多阶段构建（`Dockerfile`）：
1. `node:20-alpine` - 构建前端
2. `rust:alpine` - 编译 Rust（带依赖缓存优化）
3. `alpine:latest` - 最终镜像（包含 controller、node、client 二进制文件 + 前端静态文件）

Docker Compose 文件：
- `docker-compose.yml` - 全栈部署
- `docker-compose.controller.yml` / `docker-compose.node.yml` / `docker-compose.client.yml` - 独立组件部署

## 常见问题排查

### Controller 启动失败
- 检查端口 3000 和 3100 是否被占用
- 检查数据库文件权限
- 查看日志中的数据库迁移错误

### Node/Client 无法连接到 Controller
- 确认 Controller gRPC 服务运行在 3100 端口
- 检查 token 是否正确
- 查看 Node/Client 日志中的连接错误

### 前端构建失败
- 确保使用 Bun 1.0+
- 检查 TypeScript 类型错误
- 运行 `bun run lint` 查看代码问题
