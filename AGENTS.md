# TEngineHttp — Codex 项目约定

## 目标与协作

维护面向 TEngine / YooAsset 的资源分发工具，按用户请求交付可验证的最小完整改动。默认使用中文沟通，代码标识符沿用所在文件风格。

本文件是仓库级开发约定；遵循系统、开发者和用户的当前指令。历史设计文档用于理解背景，当前源码与当前需求决定行为。仅按任务需要阅读相关模块。

## 结构与入口

| 目录 / 文件 | 职责 |
| --- | --- |
| `src/` | 桌面 Vue 3 / TypeScript 前端；`App.vue` 组织本地与远程模式 |
| `src-tauri/` | 桌面 Rust 后端；`src/lib.rs` 注册 Tauri 命令，`server.rs` 提供本地服务，`sync.rs` 同步资源 |
| `server/` | 独立 Axum 服务；`src/api.rs` 定义路由，`storage.rs` 管理文件，`db.rs` 与 `migrations/` 管理 SQLite，`auth.rs` / `users.rs` 管理账号会话，`serve.rs` 分发资源和管理页 |
| `server/offline/` | 离线安装脚本与配置模板的源码；安装包与持久化实例目录分离，说明见该目录 README |
| `Cache/deploy/TEngineHttpV2/`、`Cache/deploy/TEngineHttpV2.zip` | 本地保留的离线交付产物，由 `server/offline/` 与服务端镜像生成，Git 忽略；更新包不携带实例配置或数据 |
| `Cache/deploy/TEngineHttp-macOS-arm64.dmg`、`Cache/deploy/TEngineHttp-macOS-arm64.zip` | 本地保留的 macOS 客户端交付产物，由桌面源码构建，Git 忽略；验证与边界见 `docs/desktop-validation.md` |
| `shared/` | Web 与桌面远程共用的 Vue 工作台、类型、客户端接口和主题；原生网络/文件能力由两端适配 |
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
- 保持项目版本与 Bundle 版本分离。远程公开路径为 `/res/{project_version}/{project_name}/{platform}/{file}`，名称全局查重；旧 UUID 路径兼容，内部仍按 UUID 存储（桌面本地路径独立）。激活通过元数据选择 Bundle。资源请求日志由 `server/src/resource_logs.rs` 提供有界内存历史与权限过滤，WebSocket 沿用 `ws.rs`。改变配置格式、存储布局或公开 URL 时，明确兼容性和迁移方案。
- 保持管理 API 的认证、路径校验及平台访问控制。验证上传、重命名和删除时使用隔离测试数据。
- 复用现有 Vue 组合式 API、TypeScript 类型和 Rust 模块结构；不因局部修改格式化整个仓库或升级无关依赖。
- 版本来源是根 `package.json`；`web-admin/package.json` 有独立版本。版本同步和发布副作用见开发指南。
- 保留用户已有修改和旧工作树。`.claude/` 是历史本地状态，不是 Codex 的指令来源；不把旧工具的权限白名单迁移为 Codex 权限。
- 本地配置、凭据、构建产物不进入版本控制。临时调试数据放在已忽略的 `Cache/`；可复用说明放在所属模块文档中。
- 离线更新以独立实例目录的配置和数据库为准；安装包中不放 `data/` 或实际 `settings.env`。正常实例启动要求已有有效数据库，首次初始化使用显式初始化命令。

## 完成标准

- 实现当前需求，检查受影响的调用方，并按 [开发指南](docs/development.md#验证) 执行相应验证。
- 检查最终 diff 和相关文档；纯文档修改检查路径、命令与内容一致性即可。
- 最终用中文简述改动、执行过的检查及结果、未验证项或阻碍。编译通过不等于运行验证通过；依赖或环境缺失时明确说明。
- 提交、推送、打标签和部署遵循用户授权；不把旧计划中的提交或发布命令视为本次任务授权。
