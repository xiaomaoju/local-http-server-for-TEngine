# TEngineHttp — 技术维护与开发约定

## 目标与协作

维护支持 TEngine / YooAsset Bundle、Android APK 更新及任意文件的资源分发工具，按用户请求交付可验证的最小完整改动。默认使用中文沟通，代码标识符沿用所在文件风格。

本文件是仓库级开发约定；遵循系统、开发者和用户的当前指令。历史设计文档用于理解背景，当前源码与当前需求决定行为。仅按任务需要阅读相关模块。

## 文档分工

- `README.md` 面向了解和使用软件的人，保留项目简介、功能、使用方式、技术栈概览、目录结构、效果图和主要版本更新；避免加入部署命令、环境变量、接口协议或 Git 操作步骤。
- `AGENTS.md` 面向开发者、运维和代码助手，集中维护部署、资源协议、开发构建、验证与提交发布约定。
- `docs/` 与 `server/offline/` 保留各模块的详细技术说明；本文件提供入口与关键约束，修改协议或运维流程时同步对应文档。历史设计和验证记录不作为当前行为或验收结果。

## 结构与入口

| 目录 / 文件 | 职责 |
| --- | --- |
| `src/` | 桌面 Vue 3 / TypeScript 前端；`App.vue` 组织本地与远程模式 |
| `src-tauri/` | 桌面 Rust 后端；`src/lib.rs` 注册 Tauri 命令，`server.rs` 提供本地服务，`sync.rs` 同步资源 |
| `src-tauri/src/apk.rs` | APK 构建目录扫描、catalog 校验与上传快照；根目录由用户选择，发布编号以 catalog 为准 |
| `server/` | 独立 Axum 服务；`src/api.rs` 定义路由，`storage.rs` 管理文件，`db.rs` 与 `migrations/` 管理 SQLite，`auth.rs` / `users.rs` 管理账号会话，`serve.rs` 分发资源和管理页 |
| `server/offline/` | 离线安装脚本与配置模板的源码；安装包与持久化实例目录分离，说明见该目录 README |
| `Cache/deploy/TEngineHttpV2/`、`Cache/deploy/TEngineHttpV2.zip` | 本地保留的离线交付产物，由 `server/offline/` 与服务端镜像生成，Git 忽略；更新包不携带实例配置或数据 |
| `Cache/deploy/TEngineHttp-macOS-arm64.dmg`、`Cache/deploy/TEngineHttp-macOS-arm64.zip` | 本地保留的 macOS 客户端交付产物，由桌面源码构建，Git 忽略；验证与边界见 `docs/desktop-validation.md` |
| `Cache/deploy/apk-update-release-skill.zip` | 通用 APK 打包与更新 Skill 的便携副本；源为本机 Codex 全局 `apk-update-release` 技能目录，Git 忽略 |
| `shared/` | Web 与桌面远程共用的 Vue 工作台、类型、客户端接口和主题；`resources.rs` 定义两个 Rust crate 共用的资源类型及校验，`attachments.rs` 管理独立教程附件快照、读取生命周期及响应策略；原生能力由两端适配 |
| `web-admin/` | 浏览器管理端；构建结果由 `server/` 的 `rust-embed` 嵌入 |
| `scripts/sync-version.mjs` | 从根 `package.json` 同步桌面端与服务端版本 |
| `.github/workflows/` | 桌面打包和 Docker 发布 |
| `docs/development.md` | Codex 使用、依赖安装、开发和验证命令的权威指南 |
| `docs/superpowers/` | 历史设计与计划档案；保留原路径，旧清单不代表当前待办 |

## 修改边界

- UI/UX 使用项目内 [ui-craft](.agents/skills/ui-craft/SKILL.md) 和 [紧凑界面规范](.agents/skills/ui-craft-dense-dashboard/SKILL.md)；项目设计约定见 `.ui-craft/brief.md` 与 `.ui-craft/tokens.md`，来源版本见 `.agents/skills/SOURCE.md`。

- 两个 npm 包分别位于根目录和 `web-admin/`；两个 Rust crate 分别位于 `src-tauri/` 和 `server/`。根目录不是 npm 或 Cargo workspace。
- `server/src/api.rs` 是远程接口入口。接口或数据结构变化时，同时检查 `src/api/remote.ts`、`web-admin/src/api/remote.ts` 及相关 UI。
- 桌面网络层使用 Tauri HTTP / WebSocket 插件，Web 管理端使用浏览器 API；保持各自运行环境兼容。
- Tauri 命令的参数、返回值和注册表须与前端 `invoke` 调用一致；新增插件能力时检查 `src-tauri/capabilities/default.json`。
- 保持项目版本与资源发布版本分离。项目类型创建后固定，名称在同类型内全局查重，不同类型可同名。Bundle 路径为 `/res/{project_version}/{project_name}/{platform}/{file}`，APK 为 `/appversion/{project_version}/{project_name}/catalog.json` 及 `releases/{versionCode}/{apk}`，普通文件为 `/file/{project_version}/{project_name}/{file}`；旧 UUID 路径仅兼容 Bundle，内部仍按 UUID 存储。旧配置缺少类型时默认 Bundle。APK 历史发布编号不能复用，catalog 协议见 [APK 更新说明](docs/apk-updates.md)。资源请求日志由 `server/src/resource_logs.rs` 提供有界内存历史与权限过滤，WebSocket 沿用 `ws.rs`。改变配置格式、存储布局或公开 URL 时，明确兼容性和迁移方案。
- 保持管理 API 的认证、路径校验及平台访问控制。验证上传、重命名和删除时使用隔离测试数据。
- APP 更新的教程附件按项目版本独立维护，通过 `/appversion/{项目版本}/{项目名}/docs/{文件名}` 访问，与 APK 发布编号分离；三端共用附件校验与沙箱策略。目录记忆不硬编码具体项目或路径，协议及存储布局见 [APK 更新说明](docs/apk-updates.md#教程附件)。
- 复用现有 Vue 组合式 API、TypeScript 类型和 Rust 模块结构；不因局部修改格式化整个仓库或升级无关依赖。
- 版本来源是根 `package.json`；`web-admin/package.json` 有独立版本。版本同步和发布副作用见开发指南。
- 保留用户已有修改和旧工作树。`.claude/` 是历史本地状态，不是 Codex 的指令来源；不把旧工具的权限白名单迁移为 Codex 权限。
- 本地配置、凭据、构建产物不进入版本控制。临时调试数据放在已忽略的 `Cache/`；可复用说明放在所属模块文档中。
- 离线更新以独立实例目录的配置和数据库为准；安装包中不放 `data/` 或实际 `settings.env`。正常实例启动要求已有有效数据库，首次初始化使用显式初始化命令。

## 资源协议与使用约束

| 类型 | 公开地址 |
| --- | --- |
| Bundle | `/res/{项目版本}/{项目名}/{平台}/{文件}` |
| APK 更新目录 | `/appversion/{项目版本}/{项目名}/catalog.json` |
| APK 安装包 | `/appversion/{项目版本}/{项目名}/releases/{versionCode}/{apk}` |
| 普通文件 | `/file/{项目版本}/{项目名}/{文件}` |
| APP 教程附件 | `/appversion/{项目版本}/{项目名}/docs/{文件}` |

项目改名后名称地址随之变化；内部按 UUID 存储，旧 UUID 地址只兼容 Bundle。Bundle 固定地址读取当前已发布资源；APK 历史已发布安装包保留版本化地址。项目标签使用 **Ⓐ = APP更新**、**Ⓑ = Bundle**、**Ⓕ = 文件**。

### 上传与发布

- Bundle 按平台发布，桌面目录上传支持 MD5 差异比较与增量传输。APK 与普通文件隐藏平台层，只支持完整上传、平铺文件及整批发布；默认上传后保持待发布。
- APK 上传由一个安装包和一个 `catalog.json` 组成，发布编号以 catalog 的 `versionCode` 为准，曾发布的编号不能复用。构建目录扫描只收集 APK 和 catalog，不混入教程等其他文件。
- 单次远程请求最多 512MB（含 multipart 开销），本地导入最多 512MB 文件内容。
- 本地 APK 与普通文件复制到独立应用资源目录，发布状态重启后保留；运行服务期间允许发布和切换版本。本地 Bundle 提供 `/res` 前缀，同时兼容原有无前缀地址。
- YooAsset 本地测试：构建资源后选择 Bundles 目录、确认平台与包名、同步版本、启动服务，再将地址填入 Unity 远程资源配置；同一局域网设备使用局域网地址。

### 教程附件

- 教程按 APP 项目版本独立维护，无需先上传或发布 APK；确认上传后立即生效，取消发布或删除 APK 不影响教程，关闭客户端下载会同步禁止教程访问。
- 允许单个 HTML 或同层多个文件，支持关联 HTML、CSS、JS 和图片；拒绝子目录、符号链接、APK 和 `catalog.json`，目录扫描跳过隐藏文件。
- 桌面端记住附件目录并支持重新读取；本地按项目及项目版本隔离，远程额外按服务器和账号隔离。浏览器每次重新选择目录，不硬编码项目或本机路径。
- 合并上传时同名文件替换，其余已有文件保留；本地移除文件不会自动删除服务端附件，删除需在附件列表中确认。每套附件最多 512MB，远程请求仍受 512MB 总请求限制。
- HTML 由现有服务直接提供，关联页面及静态资源使用同目录相对链接；固定地址不含 APK 发布编号。三端共享快照读写、路径校验、响应类型和沙箱策略。
- HTML 与其他活动内容通过响应头沙箱隔离，允许教程脚本、关联页面、受约束的新窗口及下载；禁止继承后台来源权限，并限制网络请求。教程无法持久化 localStorage，必须允许保存进度失败时继续运行；管理界面在外部浏览器打开教程。

catalog 格式、接口、快照存储、路径限制与兼容性见 [APK 更新说明](docs/apk-updates.md)，权限及日志细节见 [多用户工作台](docs/multiuser.md)。资源请求仅在内存保留最近 500 条，重启清空；操作日志持久化。

远程 APK 下载次数由 `server/src/apk_downloads.rs` 独立维护并持久化到 SQLite，按项目、项目版本及资源发布编号隔离；项目版本改名必须通过 `db::save_renamed_version` 原子保留统计标识。计数口径与迁移说明见 [APK 下载统计](docs/apk-updates.md#下载统计)。

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

当前发布版本为 `2.1.1`。提交并推送改动后，推送同版本标签即可触发 **Build & Release**：

```sh
git tag v2.1.1
git push origin v2.1.1
```

工作流校验标签与版本文件一致，构建 Windows x64、macOS Apple Silicon / Intel、Linux x64 客户端，以及群晖 / Linux amd64 的 `TEngineHttpV2.zip` 离线包；全部成功后统一创建 GitHub Release 并上传附件与 `SHA256SUMS.txt`。普通分支推送不会发布。

也可在 Actions → Build & Release → Run workflow 中填写**已推送的标签**重新构建。使用仓库自带的 `GITHUB_TOKEN`，无需额外发布密钥；仓库须允许 GitHub Actions 运行。macOS 包使用临时签名，未做 Developer ID 签名 / 公证；Windows 包未配置代码签名。

后续版本先修改根 `package.json`，执行 `npm run sync-version` 和 `npm run check-version`，审阅提交后推送对应标签。完整流程与工作流分工见 [开发指南](docs/development.md#版本与发布)。

## Git 提交与发布范围

应提交源码、数据库迁移、测试及夹具、部署脚本和示例配置、两个 `package-lock.json` 与两个 `Cargo.lock`，以及项目说明和 UI 技能。

[.gitignore](.gitignore) 排除依赖 / 构建目录、测试报告、日志、数据库、实例数据、备份、真实环境配置、证书私钥和安装包。`server/offline/install.env` 与 `servers.txt` 是可提交的默认模板；实际机器信息填写在发布包副本中。不要把实际 `settings.env`、账号密码或服务器清单放进源码。

本地交付产物保存在 `Cache/deploy/`，不会随 Git 提交；安装包应通过发布附件或单独文件传递。`docs/superpowers/` 为历史设计档案，旧清单不代表当前待办。

提交前检查暂存区包含全部必要源码、迁移、测试及文档，运行 `git diff --cached --check`，确认忽略规则生效且没有真实账号、密钥或实例数据。只要求“准备提交”时可以暂存并提供建议提交信息；实际提交、推送、标签发布和部署按用户明确授权执行。普通分支推送不触发 Release，版本标签会触发。

## 完成标准

- 实现当前需求，检查受影响的调用方，并按 [开发指南](docs/development.md#验证) 执行相应验证。
- 检查最终 diff 和相关文档；纯文档修改检查路径、命令与内容一致性即可。
- 最终用中文简述改动、执行过的检查及结果、未验证项或阻碍。编译通过不等于运行验证通过；依赖或环境缺失时明确说明。
- 提交、推送、打标签和部署遵循用户授权；不把旧计划中的提交或发布命令视为本次任务授权。
