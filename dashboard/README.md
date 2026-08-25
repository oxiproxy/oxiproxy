# OxiProxy Dashboard

OxiProxy 反向代理服务的 Web 管理面板，基于 React + TypeScript + Vite 构建。

## 功能特性

- **仪表板** - 查看系统概览统计、在线状态和流量数据
- **客户端管理** - 创建、删除客户端，查看在线状态和连接 Token
- **代理管理** - 管理代理规则（TCP/UDP），支持端口映射和流量统计
- **流量统计** - 可视化流量趋势，查看用户、客户端、代理的流量排行
- **用户管理** - 管理系统用户，分配客户端权限（管理员功能）

## 技术栈

- **React 19** - UI 框架
- **TypeScript** - 类型安全
- **Vite** - 构建工具
- **React Router** - 路由管理
- **Axios** - HTTP 请求
- **Tailwind CSS** - 样式框架
- **shadcn/ui + Radix UI** - 可组合、可访问的界面组件
- **Lucide** - 图标系统
- **Sonner** - Toast 通知

## 开发

### 安装依赖

```bash
npm install
```

### 配置后端地址

编辑 `.env` 文件：

```
VITE_API_URL=http://localhost:3000/api
```

### 启动开发服务器

```bash
npm run dev
```

访问 http://localhost:5173

### 构建生产版本

```bash
npm run build
```

构建产物将输出到项目根目录的 `dist` 文件夹。

## 项目结构

```
dashboard/
├── src/
│   ├── components/       # 通用组件
│   │   ├── Layout.tsx          # 主布局
│   │   ├── AuthShell.tsx       # 登录/注册布局
│   │   ├── ui/                 # shadcn/ui 组件
│   │   └── ProtectedRoute.tsx  # 路由守卫
│   ├── contexts/        # React Context
│   │   └── AuthContext.tsx     # 认证上下文
│   ├── lib/             # 工具库
│   │   ├── api.ts              # Axios 配置
│   │   ├── services.ts         # API 服务
│   │   ├── types.ts            # TypeScript 类型
│   │   └── utils.ts            # 工具函数
│   ├── pages/           # 页面组件
│   │   ├── Dashboard.tsx       # 仪表板
│   │   ├── Clients.tsx         # 客户端管理
│   │   ├── Proxies.tsx         # 代理管理
│   │   ├── Users.tsx           # 用户管理
│   │   ├── Traffic.tsx         # 流量统计
│   │   └── Login.tsx           # 登录页面
│   ├── App.tsx          # 应用入口
│   ├── main.tsx         # React 挂载
│   └── index.css        # 全局样式
├── .env                 # 环境变量
├── index.html           # HTML 模板
├── package.json         # 依赖配置
├── tsconfig.json        # TypeScript 配置
├── vite.config.ts       # Vite 配置
└── components.json      # shadcn/ui 配置
```

## API 接口

前端与后端 API 通信，主要接口包括：

- `POST /api/auth/login` - 用户登录
- `GET /api/dashboard/:userId` - 获取仪表板统计
- `GET /api/clients` - 获取客户端列表
- `POST /api/clients` - 创建客户端
- `DELETE /api/clients/:id` - 删除客户端
- `GET /api/proxies` - 获取代理列表
- `POST /api/proxies` - 创建代理
- `PUT /api/proxies/:id` - 更新代理
- `DELETE /api/proxies/:id` - 删除代理
- `GET /api/users` - 获取用户列表（管理员）
- `GET /api/traffic/overview` - 获取流量统计
