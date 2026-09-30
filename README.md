# TEngineHttp 2.0

面向 Unity [TEngine](https://github.com/ALEXTANGXIAO/TEngine) / [YooAsset](https://github.com/tuyoogame/YooAsset) 的资源管理与分发工具。通过桌面客户端或浏览器完成 **创建项目 → 上传资源 → 发布版本 → 复制下载地址**，支持独立账号、项目共享和操作追溯。

桌面基于 Tauri 2、Vue 3 与 TypeScript；服务端采用 Rust、Axum、SQLite，Web 管理页随服务端一起部署。

## 使用方式

| 方式 | 适用场景 | 主要能力 |
| --- | --- | --- |
| 桌面本地 | 开发调试、局域网真机测试 | 选择 YooAsset 构建目录、同步版本、启动本机 HTTP 服务 |
| 桌面远程 | 向服务器或 NAS 发布资源 | 保存多个服务器连接、目录扫描、完整 / 增量上传、版本发布 |
| Web 管理端 | 多人协作和后台管理 | 浏览器上传、项目共享、用户管理、发布与操作记录 |

三种方式使用统一的紧凑布局，支持深浅主题。平台支持 Android、iOS、Windows、macOS、Linux、WebGL。

## 主要功能

- **账号与权限**：管理员创建账号、重置密码、禁用和删除用户；首次登录与密码重置后强制改密。删除用户前转交其项目，资源保留。
- **项目协作**：所有用户都能创建项目，可共享给其他账号；服务端按项目检查配置、上传和日志权限。
- **两层版本**：项目版本（如 `v1`、`v2`）隔离不同客户端的资源；每个平台下保存多个资源版本（Bundle），发布时选择客户端下载的版本。
- **上传与发布**：默认上传完成后手动发布，可切换到已上传的历史版本；桌面目录上传支持 MD5 差异比较和增量传输，也可按项目及平台启用自动发布。
- **下载控制**：每个平台独立开启或关闭客户端下载；管理接口需要登录，游戏资源下载保持公开。
- **文件与日志**：查看版本文件、复制地址；操作日志和资源请求分开显示，通过 WebSocket 实时更新。操作记录持久化，资源请求仅在内存保留最近 500 条，重启清空。
- **桌面连接**：保存服务器地址、用户名和构建目录；macOS / Windows 可选择将密码存入系统凭据库，会话令牌只保存在内存中。

| 操作 | 普通用户 | 系统管理员 |
| --- | --- | --- |
| 创建项目 | 可以 | 可以 |
| 配置、上传、发布、删除资源版本 | 自己拥有或共享给自己的项目 | 所有项目 |
| 管理共享、转交、删除整个项目 | 自己拥有的项目 | 所有项目 |
| 管理用户与系统设置 | 不可以 | 可以 |

权限与接口细节见 [多用户工作台](docs/multiuser.md)。

## 界面预览

保存服务器连接与登录：

![保存服务器连接与登录](docs/screenshot.png)

项目工作台、版本发布与下载控制：

![项目工作台、版本发布与下载控制](docs/screenshot2.png)

本地构建目录与增量上传：

![本地构建目录与增量上传](docs/screenshot3.png)

项目操作记录：

![项目操作记录](docs/screenshot4.png)

Web 用户管理：

![Web 用户管理](docs/screenshot5.png)

## 日常使用

### 本地资源测试

1. 用 YooAsset 构建资源，打开桌面客户端的本地模式。
2. 创建项目，选择 Bundles 目录，确认平台与包名。
3. 同步资源版本，启动服务。
4. 复制访问地址，填入 Unity 的远程资源配置；同一局域网设备可使用局域网地址访问。

### 远程上传与发布

1. 登录 Web 或桌面远程模式；首次登录先修改初始密码。
2. 创建或打开项目，选择项目版本与平台。
3. Web 选择资源文件；桌面选择构建目录，确认完整或增量上传的目标与差异。
4. 上传完成后点击“发布此版本”，确认即将替换的版本；默认不会自动发布。
5. 确认已开启客户端下载，复制工作台提供的下载地址。

公开资源路径：

```text
https://your-domain/res/{项目版本}/{项目名}/{平台}/{文件}
https://your-domain/res/v1/MyGame/Android/DefaultPackage.version
```

项目名称全局查重，改名会改变名称下载地址。内部仍按 UUID 存储，已有 UUID 下载链接继续兼容。项目版本与资源版本的含义不同，下载地址由当前发布的资源版本提供内容。

## 远程服务（Docker）

默认服务端口为 **8083**，可与使用 8082 的旧版并行运行。生产环境通过 HTTPS 访问；可使用群晖 / Nginx 反向代理，也可配置服务端 TLS。

### 离线安装：群晖或 Linux 服务器

`TEngineHttpV2` 离线包包含 `linux/amd64` 镜像，适用于 DS918+ 等 Intel / AMD 64 位设备；ARM 设备需要另行构建对应架构镜像。安装脚本与模板源码位于 [server/offline](server/offline/README.md)。

1. 将安装包解压到临时目录，例如 Downloads。
2. 编辑包内 `install.env`，设置独立的 `INSTANCE_DIR`。默认 `/volume3/docker/TEngineHttpV2`，按实际存储卷修改，不能放在安装包目录内。
3. 首次安装可编辑 `settings.env.example`：配置 `PORT=8083`、初始用户名等。密码留空时由脚本生成。
4. 在安装包目录通过 SSH 执行：

   ```sh
   sudo sh deploy.sh
   ```

初始账号和密码保存在 **实例目录的 `settings.env`**，以该文件为准。首次登录要求改密；以后修改初始化变量不会重置已有账号。

群晖反向代理设置 HTTPS 入口，目标为 HTTP `127.0.0.1:8083`，并添加 WebSocket 标头；`COOKIE_SECURE=true` 保持不变。其他服务器可参考 [Nginx 配置示例](server/nginx.conf.example)。

更新时将新包解压到临时目录，指向原实例，再运行同一命令。确认更新后先停服备份，再替换程序，保留配置、账号与资源。多台服务器填写包内 `servers.txt`，在 macOS / Linux 发布电脑执行 `bash batch-deploy.sh`。

完整步骤见 [精简发布指南](server/offline/发布指南.md) 和 [离线安装、迁移与恢复说明](server/offline/README.md)。

### 从源码部署

在仓库中执行：

```sh
cd server
cp .env.example .env
```

编辑 `.env`，设置自己的 `BOOTSTRAP_ADMIN_USERNAME`、强密码 `BOOTSTRAP_ADMIN_PASSWORD`，并加入 `REQUIRE_EXISTING_DB=true`。配置 HTTPS 入口；仅隔离的本机 HTTP 测试设 `COOKIE_SECURE=false`。

全新实例只执行一次初始化：

```sh
docker compose build
docker compose run --rm --no-deps tengine-server tengine-server --initialize
docker compose up -d
```

后续更新只执行 `docker compose up -d --build`，不要重复初始化。此方式使用 `tengine-management-data` 命名卷，与离线安装的目录挂载不同；勿执行会删除数据卷的清理操作。无需额外部署 Web 服务，管理页已嵌入镜像。

### 数据与备份

离线安装将配置与数据保存在固定实例目录中，安装包可在部署完成后删除：

```text
TEngineHttpV2/                  # INSTANCE_DIR 指定的运行实例
├── settings.env               # 实际配置与初始账号信息
├── docker-compose.yml
├── data/
│   ├── management.sqlite      # 用户、会话、项目、权限与操作记录
│   └── resources/             # 上传的资源文件
└── backups/                   # 更新前的数据与配置备份
```

保留实例目录并重新挂载，重建容器或重装 Docker 后仍可使用原账号与资源。手动备份应先停服，再复制整个实例目录到其他磁盘；恢复时数据库、资源和配置一起恢复，不单独复制运行中的 SQLite 主文件。

离线运行实例启用 `REQUIRE_EXISTING_DB=true`，数据库丢失时拒绝自动新建账号。旧单密码 / `config.json` 系统不直接导入；已有多用户旧目录的迁移由离线脚本处理，详见安装说明。

### 环境变量

以下为服务端配置。离线安装另有 `INSTANCE_DIR`、`BIND_ADDRESS`，见带注释的 [安装目录配置](server/offline/install.env) 和 [首次安装模板](server/offline/settings.env.example)。

| 变量 | 默认值 | 说明 |
| --- | --- | --- |
| `BOOTSTRAP_ADMIN_USERNAME` | 无 | 首次初始化管理员用户名；已有账号不受影响 |
| `BOOTSTRAP_ADMIN_PASSWORD` | 无 | 首次初始化密码；离线脚本可自动生成，直接运行服务端需提供 |
| `REQUIRE_EXISTING_DB` | `false` | 正常部署设为 `true`，缺少有效数据库时拒绝启动；离线实例已固定开启 |
| `PORT` | `8083` | HTTP 监听端口；离线脚本同时用于宿主机映射与健康检查 |
| `DATA_DIR` | `/data` | 数据库和资源根目录，同一目录只允许一个服务进程使用 |
| `COOKIE_SECURE` | `true` | Web Cookie 仅通过 HTTPS 发送；反向代理到后端为 HTTP 时也保持 `true` |
| `TOKEN_EXPIRE_HOURS` | `24` | 会话有效期，允许 1–168 小时 |
| `TLS_CERT` / `TLS_KEY` | 无 | PEM 证书与私钥路径，两者同时提供时启用原生 HTTPS |
| `HTTPS_PORT` | `8182` | 原生 HTTPS 监听端口 |

## 开发与构建

### 环境要求

- Node.js 20（与当前 CI / Docker 一致）及 npm。
- Rust 工具链；服务端 Docker 固定 Rust 1.90，桌面 CI 使用 stable。
- 桌面构建依赖：Windows 的 C++ Build Tools、macOS 的 Xcode Command Line Tools；Linux 依赖见 [桌面构建工作流](.github/workflows/release.yml)。
- 构建或运行容器时需要 Docker。

从仓库根目录安装两个前端的依赖：

```sh
npm ci
npm --prefix web-admin ci
```

| 任务 | 命令 |
| --- | --- |
| 桌面开发 | `npm run tauri dev` |
| 桌面前端检查与构建 | `npm run build` |
| 桌面打包 | `npm run tauri build` |
| Web 管理端构建 | `npm --prefix web-admin run build` |
| 服务端测试 | `cargo test --locked --manifest-path server/Cargo.toml` |
| 桌面 Rust 检查 | `cargo check --locked --manifest-path src-tauri/Cargo.toml` |
| 浏览器回归 | `npm --prefix web-admin run test:e2e` |

服务端编译 / 测试前先构建 Web 管理端；浏览器回归还需安装 Playwright Chromium。开发服务启动、测试隔离、原生依赖及版本同步详见 [开发指南](docs/development.md)。`npm version` 会触发版本同步及 Git 提交 / 标签，不作为普通提交前检查。

目前已有 macOS arm64 原生验证和 Linux amd64 容器验证；其他桌面平台的本轮运行边界见 [桌面验证记录](docs/desktop-validation.md)，服务端与浏览器记录见 [多用户验证记录](docs/multiuser-validation.md)。历史验证不代替后续改动的检查。

### GitHub 自动发布

当前发布版本为 `2.0.0`。提交并推送改动后，推送同版本标签即可触发 **Build & Release**：

```sh
git tag v2.0.0
git push origin v2.0.0
```

工作流校验标签与版本文件一致，构建 Windows x64、macOS Apple Silicon / Intel、Linux x64 客户端，以及群晖 / Linux amd64 的 `TEngineHttpV2.zip` 离线包；全部成功后统一创建 GitHub Release 并上传附件与 `SHA256SUMS.txt`。普通分支推送不会发布。

也可在 Actions → Build & Release → Run workflow 中填写**已推送的标签**重新构建。使用仓库自带的 `GITHUB_TOKEN`，无需额外发布密钥；仓库须允许 GitHub Actions 运行。macOS 包使用临时签名，未做 Developer ID 签名 / 公证；Windows 包未配置代码签名。

后续版本先修改根 `package.json`，执行 `npm run sync-version` 和 `npm run check-version`，审阅提交后推送对应标签。完整流程与工作流分工见 [开发指南](docs/development.md#版本与发布)。

## 仓库结构与提交范围

```text
src/                    桌面 Vue 前端与原生网络适配
src-tauri/              Tauri 命令、本地服务、目录同步与系统凭据
shared/                 双端共用工作台、组件、类型与主题
web-admin/              浏览器适配、构建入口与 Playwright 测试
server/src/             Axum API、认证、资源服务与日志
server/migrations/      SQLite 数据库迁移
server/offline/         离线安装脚本与无真实密码的配置模板
.agents/skills/          项目内 UI Craft 技能、许可证与固定来源版本
.ui-craft/              界面设计说明与主题约定
docs/                   开发、部署与验证说明
Cache/                  本地安装包、测试数据与调试产物（不提交）
```

应提交源码、数据库迁移、测试及夹具、部署脚本和示例配置、两个 `package-lock.json` 与两个 `Cargo.lock`，以及项目说明和 UI 技能。

[.gitignore](.gitignore) 排除依赖 / 构建目录、测试报告、日志、数据库、实例数据、备份、真实环境配置、证书私钥和安装包。`server/offline/install.env` 与 `servers.txt` 是可提交的默认模板；实际机器信息填写在发布包副本中。不要把实际 `settings.env`、账号密码或服务器清单放进源码。

本地交付产物保存在 `Cache/deploy/`，不会随 Git 提交；安装包应通过发布附件或单独文件传递。项目开发约定见 [AGENTS.md](AGENTS.md)。`docs/superpowers/` 为历史设计档案，旧清单不代表当前待办。

## License

MIT。项目内 UI Craft 的来源与许可证见 [.agents/skills/SOURCE.md](.agents/skills/SOURCE.md)。
