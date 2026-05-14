# TEngine Http Server

Unity [TEngine](https://github.com/ALEXTANGXIAO/TEngine) + [YooAsset](https://github.com/tuyoogame/YooAsset) 热更新资源管理工具。

Tauri 2 + Vue 3 + Axum 构建，支持本地服务和远程 Docker 部署。

![screenshot](docs/screenshot.png)
![screenshot](docs/screenshot2.png)
![screenshot](docs/screenshot3.png)
![screenshot](docs/screenshot4.png)

---

## 功能

- **本地模式** — 本机 / 局域网 HTTP 资源服务，自动检测 IP
- **远程模式** — Docker 部署到云服务器 / NAS，浏览器管理
- 多项目 / 多版本并行，6 平台支持
- 增量上传（MD5 比对），激活仅切元数据不拷文件
- 平台访问开关，实时日志推送
- SHA-256 + Argon2 + JWT 认证，可选 HTTPS

---

## 技术栈

| 层级 | 技术 |
|------|------|
| 桌面客户端 | Vue 3 + TypeScript + Tauri 2 |
| 远程后端 | Rust + Axum + Tokio |
| Web 管理界面 | Vue 3（rust-embed 嵌入二进制） |
| 部署 | Docker 多阶段构建 |

---

## 快速开始

### 环境要求

- Node.js >= 18、Rust >= 1.85
- Windows 需 VS Build Tools，macOS 需 Xcode CLI Tools
- 远程部署需 Docker

### 桌面客户端

```bash
npm install
npm run tauri dev          # 开发
npm run tauri build        # 打包
```

### 远程服务

**方式一：服务器构建**

```bash
cd server
cp .env.example .env && vim .env
docker compose up -d --build
```

**方式二：本地构建上传（推荐）**

```bash
cd server
bash publish.sh                        # 默认服务器
bash publish.sh user@1.2.3.4           # 指定服务器
bash publish.sh user@1.2.3.4 /opt/tengine  # 指定目录
```

首次部署需在服务器运行 `bash deploy.sh` 设置管理密码。

**方式三：群晖 NAS**

```bash
docker load < tengine-server-amd64.tar.gz
docker run -d --name tengine -p 8082:8082 -v tengine-data:/data \
  -e ADMIN_PASSWORD=你的密码 -e JWT_SECRET=$(openssl rand -hex 32) \
  --restart always tengine-server:latest
```

### 环境变量

| 变量 | 必填 | 默认 | 说明 |
|------|------|------|------|
| `ADMIN_PASSWORD` | ✅ | — | 管理密码 |
| `JWT_SECRET` | ❌ | 随机 | JWT 签名密钥，生产环境务必固定 |
| `PORT` | ❌ | `8082` | HTTP 端口 |
| `DATA_DIR` | ❌ | `/data` | 数据目录 |
| `TOKEN_EXPIRE_HOURS` | ❌ | `24` | JWT 过期时间（小时） |
| `TLS_CERT` / `TLS_KEY` | ❌ | — | TLS 证书和私钥路径，提供后启用 HTTPS |
| `HTTPS_PORT` | ❌ | `8182` | HTTPS 端口 |
| `CORS_ORIGINS` | ❌ | `*` | 允许的跨域来源，逗号分隔 |

### 版本号同步

```bash
npm version 0.0.5    # 自动同步所有文件 + 提交 + tag
```

---

## 使用流程

### 本地模式

1. Unity YooAsset 构建产物到 Bundles 目录
2. 客户端配置项目名、平台、端口
3. 选目录 → 同步资源 → 启动服务
4. Unity `IRemoteServices` 填入显示的 URL

### 远程模式

1. 部署 Docker 到服务器
2. 客户端填地址 + 密码连接
3. 建项目 / 项目版本，选目录 + 平台同步上传
4. Unity 接入 `http://<服务器>:<端口>/res/{项目版本}/{项目名}/{平台}/`

---

## API

资源下载 `/res/{project_version}/{project_name}/{platform}/{file}` 无需认证。

| 方法 | 路径 | 说明 |
|------|------|------|
| GET | `/` | Web 管理界面 |
| GET | `/res/...` | 资源下载 |
| GET | `/api/health` | 健康检查 |
| POST | `/api/auth/login` | 登录 |
| GET/POST | `/api/projects` | 项目列表 / 创建 |
| PUT/DELETE | `/api/projects/:id` | 更新 / 删除项目 |
| POST | `.../project-versions/:pver/upload` | 上传（512MB 限制） |
| POST | `.../project-versions/:pver/incremental-upload` | 增量上传 |
| PUT | `.../versions/:ver/activate?platform=X` | 激活版本 |
| WebSocket | `/api/ws/logs?token=<jwt>` | 实时日志 |

完整 API 见 `server/src/api.rs`。

---

## 项目结构

```
TEngineHttp/
├── src/                  # Tauri 客户端前端（Vue 3）
├── src-tauri/            # Tauri 客户端后端（Rust）
├── server/               # 远程服务（Axum）
│   ├── src/              # main / api / auth / storage / ws / serve
│   ├── Dockerfile        # 多阶段构建
│   ├── publish.sh        # 本地构建 + 上传部署
│   └── deploy.sh         # 服务器首次初始化
├── web-admin/            # Web 管理界面
└── scripts/              # 版本同步等工具脚本
```

---

## 生产部署建议

- 推荐用 Nginx / 宝塔面板反向代理，参考 `server/bt-nginx-rules.conf`
- 配置 `limit_req` 防止接口被刷，登录接口 1r/s，资源接口 20r/s
- 有域名建议上 Let's Encrypt SSL
- 面向中国用户建议用国内服务器

---

## License

MIT
