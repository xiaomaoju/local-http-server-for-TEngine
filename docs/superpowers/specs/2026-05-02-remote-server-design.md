# TEngine Http Server — 远程资源分发服务设计

## 概述

将现有的本地 HTTP 资源服务器扩展为支持远程部署的资源分发服务。在保留本地模式全部功能的基础上，新增远程模式：后端为独立的 Axum HTTP 服务（Docker 部署），前端通过 Tauri 客户端或浏览器 Web 管理界面进行管理。

### 核心需求

- 保留现有本地模式功能不变
- 新增远程模式：通过 REST API 管理远端资源服务器
- 后端 Docker 一键部署，自带 Web 管理界面
- 管理接口密码保护（SHA-256 + argon2 + JWT），资源下载公开
- 实时日志推送（WebSocket）
- 前后端完全解耦，独立开发、独立部署

---

## 1. 系统架构

```
┌──────────────────┐     ┌──────────────────┐
│  Tauri 桌面客户端  │     │  浏览器            │
│  本地模式 | 远程模式│     │  Web 管理界面       │
└────────┬─────────┘     └────────┬─────────┘
         │                        │
         └───────┬────────────────┘
                 ▼  同一套 REST API
┌─────────────────────────────────────┐
│  Docker 容器 — Axum 服务             │
│  ┌────────────────────────────────┐ │
│  │  /          Web 管理界面(SPA)   │ │
│  │  /api/*     管理接口(JWT保护)   │ │
│  │  /res/*     资源分发(公开)      │ │
│  │  /api/ws/*  WebSocket日志(JWT) │ │
│  └────────────────────────────────┘ │
│  /data/  持久化存储 (Docker Volume)  │
│    ├─ config.json                   │
│    └─ resources/                    │
└─────────────────────────────────────┘
```

Tauri 客户端和 Web 管理界面功能完全一致，都通过同一套 REST API 操作后端。后端是纯 Axum HTTP 服务，不依赖 Tauri，独立编译部署。

---

## 2. 代码结构

```
TEngineHttp/
├── src/                          # Tauri 前端 (Vue 3) — 改造
│   ├── App.vue                   # 模式切换容器
│   ├── components/
│   │   ├── LocalMode.vue         # 现有本地模式逻辑提取
│   │   ├── RemoteMode.vue        # 远程模式主界面
│   │   ├── RemoteLogin.vue       # 远程登录（地址+密码）
│   │   ├── ProjectTabs.vue       # 项目标签页（共用）
│   │   ├── LogPanel.vue          # 日志面板（共用）
│   │   └── FileUploader.vue      # 文件上传组件（远程专用）
│   ├── api/
│   │   └── remote.ts             # 远程 API 封装 (fetch + JWT)
│   ├── main.ts
│   └── styles/
├── src-tauri/                    # Tauri 后端 — 小幅改造
│   └── src/
│       ├── lib.rs                # 增加远程相关 Tauri 命令
│       ├── server.rs             # 不变（本地模式用）
│       ├── sync.rs               # 不变（本地模式用）
│       ├── config.rs             # 增加远程连接配置持久化
│       └── remote.rs             # 新增：远程 API 客户端封装
├── server/                       # 新增：独立后端
│   ├── Cargo.toml
│   ├── Dockerfile
│   ├── docker-compose.yml
│   ├── .env.example
│   └── src/
│       ├── main.rs               # Axum 入口
│       ├── api.rs                # 管理 API 路由
│       ├── auth.rs               # 认证（argon2 + JWT）
│       ├── storage.rs            # 资源存储管理
│       ├── serve.rs              # 静态资源分发
│       └── config.rs             # 服务端配置
├── web-admin/                    # 新增：Web 管理界面
│   ├── package.json              # Vue 3 + Vite
│   ├── src/
│   │   ├── App.vue               # 管理界面（与 RemoteMode 功能一致）
│   │   ├── main.ts
│   │   ├── api/
│   │   │   └── remote.ts         # API 调用封装（与 Tauri 前端共用逻辑）
│   │   └── components/
│   │       ├── RemoteLogin.vue
│   │       ├── ProjectTabs.vue
│   │       ├── LogPanel.vue
│   │       └── FileUploader.vue
│   └── ...
```

---

## 3. 认证方案

### 登录流程

```
客户端                                    服务端
  │                                        │
  │  POST /api/auth/login                  │
  │  { "password": "<sha256_hash>" }       │
  │  (客户端对明文做 SHA-256 后发送)         │
  ├───────────────────────────────────────→ │
  │                                        ├─ 对 SHA-256 值做 argon2 验证
  │                                        ├─ 验证通过，生成 JWT token
  │  { "token": "eyJhbG..." }              │
  │ ←──────────────────────────────────────┤
  │                                        │
  │  后续管理请求:                           │
  │  Authorization: Bearer <jwt_token>     │
  ├───────────────────────────────────────→ │
  │                                        ├─ 验证 JWT 签名和有效期
```

### 要点

- 服务端启动时，读取 `ADMIN_PASSWORD` 环境变量，对其做 SHA-256 再做 argon2 哈希，存储在内存中
- 客户端登录时，对用户输入的明文密码做 SHA-256 后发送，不传明文
- 服务端收到 SHA-256 值后，与内存中的 argon2 哈希做验证
- 登录成功返回 JWT token，后续请求用 token 认证
- JWT 签名密钥通过 `JWT_SECRET` 环境变量配置（不配则启动时随机生成）
- 生产环境强烈建议配合 HTTPS

---

## 4. API 接口

### 公开接口（无需认证）

| 方法 | 路径 | 说明 |
|------|------|------|
| GET | `/` | Web 管理界面 (SPA) |
| GET | `/res/<project>/<platform>/<file>` | 资源文件下载 |
| GET | `/api/health` | 健康检查 |
| POST | `/api/auth/login` | 登录获取 JWT token |

### 管理接口（JWT 保护）

**项目管理：**

| 方法 | 路径 | 说明 |
|------|------|------|
| GET | `/api/projects` | 获取所有项目列表 |
| POST | `/api/projects` | 创建新项目 |
| PUT | `/api/projects/:id` | 更新项目配置 |
| DELETE | `/api/projects/:id` | 删除项目及其所有资源 |

**资源与版本管理：**

| 方法 | 路径 | 说明 |
|------|------|------|
| POST | `/api/projects/:id/upload` | 上传资源包 (multipart/form-data) |
| GET | `/api/projects/:id/versions` | 列出所有版本 |
| PUT | `/api/projects/:id/versions/:ver/activate` | 激活指定版本 |
| DELETE | `/api/projects/:id/versions/:ver` | 删除指定版本 |
| GET | `/api/projects/:id/status` | 获取当前状态和激活版本 |

**实时日志：**

| 方法 | 路径 | 说明 |
|------|------|------|
| WebSocket | `/api/ws/logs?token=<jwt>` | 实时日志推送 |

### 资源上传请求格式

```
POST /api/projects/:id/upload
Content-Type: multipart/form-data
Authorization: Bearer <jwt_token>

Fields:
  - platform: "Android"                     # 目标平台
  - version: "1.0.1"                        # 可选，不填则自动生成时间戳
  - files: [file1, file2, ...]              # .bundle 和 .version 文件
```

### 资源下载 URL 格式

Unity 客户端直接访问：

```
http://<server>:8080/res/TEngine/Android/DefaultPackage.version
http://<server>:8080/res/TEngine/Android/xxx.bundle
```

### WebSocket 日志消息格式

```json
{
  "timestamp": "14:23:05",
  "type": "request",
  "status": 200,
  "method": "GET",
  "path": "/res/TEngine/Android/xxx.bundle",
  "project_id": "uuid",
  "message": ""
}
```

`type` 枚举：`request`（HTTP 请求）、`upload`（上传操作）、`sync`（版本激活）、`system`（系统事件）

---

## 5. 服务端数据结构

### 存储目录

```
/data/
├── config.json
└── resources/
    └── TEngine/                             # 项目名
        ├── Android/                         # 平台
        │   ├── DefaultPackage.version       # 当前激活版本的文件（分发用）
        │   ├── xxx.bundle
        │   └── _versions/                   # 所有上传的版本存档
        │       ├── 1.0.0/
        │       │   ├── DefaultPackage.version
        │       │   └── xxx.bundle
        │       └── 1.0.1/
        │           ├── DefaultPackage.version
        │           └── xxx.bundle
        └── iOS/
            └── ...
```

### 服务端配置 (config.json)

```json
{
  "projects": [
    {
      "id": "uuid",
      "project_name": "TEngine",
      "platforms": ["Android", "iOS"],
      "package_name": "DefaultPackage",
      "active_versions": {
        "Android": "1.0.1",
        "iOS": "1.0.0"
      }
    }
  ]
}
```

### 版本激活逻辑

激活版本时，将 `_versions/<version>/` 目录下所有文件复制到平台根目录（覆盖旧文件），复用现有 `sync.rs` 的同步逻辑。Unity 客户端下次请求 `/res/` 即获取新版本资源。

---

## 6. 前端 UI 改造

### Tauri 客户端

顶部增加模式切换 `[本地模式] [远程模式]`，两种模式互不干扰。

**本地模式：** 现有功能完全保留，逻辑提取到 `LocalMode.vue`。

**远程模式 (`RemoteMode.vue`)：**

- 连接配置：服务器地址 + 密码，登录后保持 JWT
- 项目管理：通过 REST API 进行 CRUD
- 资源上传：选择本地文件，multipart 上传到远端
- 版本管理：查看版本列表，激活/删除版本
- 实时日志：WebSocket 连接，日志面板样式与本地模式一致

**组件复用：** `ProjectTabs.vue` 和 `LogPanel.vue` 在本地/远程模式间共用，通过 props 切换数据源。

### Web 管理界面 (web-admin/)

独立 Vue 3 + Vite 项目，功能与 Tauri 远程模式完全一致。不依赖 `@tauri-apps/api`，纯浏览器环境运行。API 基础地址为当前页面 origin。

构建产物通过 `rust-embed` 嵌入 Axum 二进制，单容器部署无需额外 Web 服务器。

### 本地模式 vs 远程模式对比

| 功能 | 本地模式 | 远程模式 |
|------|---------|---------|
| 项目管理 | Tauri invoke | REST API |
| 资源同步 | 本地目录 + 符号链接 | 文件选择 + HTTP 上传 |
| 启停服务 | 本地 Axum 进程管理 | 不需要（远端常驻） |
| 版本切换 | 本地 sync | API 激活 |
| 日志 | Tauri 事件 | WebSocket |
| Bundles 目录 | 本地路径选择 | 不需要 |

---

## 7. Docker 部署

### Dockerfile（多阶段构建）

```
阶段 1 (node:20-alpine): 构建 web-admin
  → npm ci && npm run build → 产出 dist/

阶段 2 (rust:1.78-bookworm): 构建 server
  → 将 web-admin/dist/ 复制进来
  → cargo build --release → 产出单个可执行文件

阶段 3 (debian:bookworm-slim): 运行
  → 复制二进制
  → EXPOSE 8080
  → 入口: ./tengine-server
```

### docker-compose.yml

```yaml
services:
  tengine-server:
    build: .
    ports:
      - "8080:8080"
    volumes:
      - tengine-data:/data
    env_file:
      - .env
    restart: unless-stopped

volumes:
  tengine-data:
```

### 环境变量

| 变量 | 必填 | 默认值 | 说明 |
|------|------|--------|------|
| `ADMIN_PASSWORD` | 是 | — | 管理密码（原始明文，服务端自动 argon2 哈希） |
| `JWT_SECRET` | 否 | 随机生成 | JWT 签名密钥 |
| `PORT` | 否 | `8080` | 监听端口 |
| `DATA_DIR` | 否 | `/data` | 数据持久化目录 |
| `TOKEN_EXPIRE_HOURS` | 否 | `24` | JWT 过期时间（小时） |

### .env.example

```env
ADMIN_PASSWORD=changeme
# JWT_SECRET=your_random_secret
# PORT=8080
# DATA_DIR=/data
# TOKEN_EXPIRE_HOURS=24
```

### 部署流程

```bash
git clone <repo>
cd TEngineHttp/server
cp .env.example .env
# 编辑 .env，设置 ADMIN_PASSWORD
docker compose up -d
# 访问 http://<服务器IP>:8080 打开 Web 管理界面
```

### 数据持久化

Docker Volume 挂载 `/data`，包含 `config.json` 和 `resources/` 目录。容器销毁重建后数据不丢失。

---

## 8. 技术选型（后端新增依赖）

| 用途 | crate |
|------|-------|
| HTTP 框架 | axum 0.7 |
| 密码哈希 | argon2 |
| JWT | jsonwebtoken |
| WebSocket | axum 内置 (ws feature) |
| 文件上传 | axum-multipart |
| 静态文件嵌入 | rust-embed |
| 序列化 | serde + serde_json |
| 异步运行时 | tokio |
| 日志 | tracing + tracing-subscriber |
| SHA-256 验证 | sha2 |
