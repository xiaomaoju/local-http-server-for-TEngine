# Codex 开发指南

## 项目入口

本仓库使用根目录 [AGENTS.md](../AGENTS.md) 作为 Codex 项目指令。Codex 按目录加载 `AGENTS.md`，规则说明见 [OpenAI 官方文档](https://learn.chatgpt.com/docs/agent-configuration/agents-md)。

在 Codex 桌面应用中打开仓库根目录，并从该目录开始新对话；CLI 则在根目录执行：

```bash
codex
```

仓库不强制指定模型、个人插件或审批模式，沿用当前 Codex 环境配置。无需安装 Claude Code 或 Superpowers，也无需复制 `.claude/settings.local.json`。已有会话若未加载新增的项目指令，可重新开始一个会话。

## 依赖与安装

以下命令均从仓库根目录运行。Node 20 与现有 CI / Docker 构建一致；远程服务的 Dockerfile 使用 Rust 1.90，桌面 CI 使用 stable。桌面开发还需要当前操作系统对应的 Tauri 原生构建依赖，参见 [README](../README.md#环境要求) 和 `.github/workflows/release.yml`。

```bash
npm ci
npm --prefix web-admin ci
```

两处安装各自使用已提交的 `package-lock.json`。Rust 依赖分别使用两个 crate 的 `Cargo.lock`，没有根级 Cargo workspace。

## 开发

### 桌面应用

```bash
npm run tauri dev
```

Tauri 自动启动 Vite（端口 1420）。`npm run dev` 只启动前端；需要原生 API 的目录选择、HTTP、WebSocket 等功能应在 Tauri 窗口中验证。

### 远程服务与 Web 管理端

服务端嵌入 `web-admin/dist`。先构建管理端，再编译服务端：

```bash
npm --prefix web-admin run build
cargo build --locked --manifest-path server/Cargo.toml
```

`server/build.rs` 在缺少 `dist` 时只创建空目录；这能让编译继续，但不会产生管理页面。发布前必须完成真实的 Web 构建。

在运行服务的终端中设置 `BOOTSTRAP_ADMIN_USERNAME`、`BOOTSTRAP_ADMIN_PASSWORD`、`DATA_DIR`、本地 HTTP 使用的 `COOKIE_SECURE=false` 及可选 `PORT`，然后执行：

```bash
cargo run --locked --manifest-path server/Cargo.toml
```

本地测试将 `DATA_DIR` 指向仓库内独立的 `Cache/debug/server-data`，避免使用已有资源目录。环境变量定义见 [README](../README.md#环境变量)；原生 `cargo run` 不会自动加载 `.env`，Compose 的 `.env` 流程不能直接套用。

默认服务地址是 `http://localhost:8083`。需要 Web 热重载时，在另一个终端执行：

```bash
npm --prefix web-admin run dev
```

Vite 配置端口为 5174，`/api` 与 `/res` 代理到 `localhost:8083`。若改变后端端口，同步检查 `web-admin/vite.config.ts`。

## 验证

按变更范围选择检查；纯文档或 AI 指令调整无需完整打包。Web 管理端提供 `test:e2e` 浏览器测试；两个前端暂无 `lint` 脚本，`build` 会执行 `vue-tsc --noEmit` 和 Vite 构建；服务端包括存储单元测试及账号、权限、上传发布接口测试。

| 变更范围 | 从根目录运行的检查 |
| --- | --- |
| 桌面 Vue / TypeScript | `npm run build` |
| Web 管理端 | `npm --prefix web-admin run build` |
| 桌面 Rust / Tauri 命令 | `cargo check --locked --manifest-path src-tauri/Cargo.toml` |
| 服务端 Rust / 存储 | 先构建 Web 管理端，再运行 `cargo test --locked --manifest-path server/Cargo.toml` |
| 远程接口或共享数据模型 | 两个前端构建、服务端测试；涉及 Tauri 原生命令时加桌面 Rust 检查 |
| 桌面发布包 | `npm run tauri build`，并在目标操作系统验证 |
| Docker 构建逻辑 | `docker build -f server/Dockerfile -t tengine-server:local .` |
| 文档 / 项目指令 | 检查引用路径、命令是否存在及 `git diff --check` |

修改运行行为后，补充与变更相关的手动路径验证。例如登录及认证拒绝、项目 / 项目版本创建、上传与激活、资源下载、平台关闭返回 403、日志推送。只运行受影响的路径，并记录实际结果。测试数据与生产资源隔离。

`cargo check`、`cargo test` 或前端构建通过，不能代替桌面交互、嵌入页面、Docker 运行或跨平台打包验证。缺少工具或依赖时，报告命令、阻碍和未验证范围，不宣称通过。

## 版本与发布

根 `package.json` 是发布版本来源，`npm run sync-version` 更新：

- `server/Cargo.toml`
- `server/Cargo.lock` 中的应用自身版本
- `src-tauri/Cargo.toml`
- `src-tauri/Cargo.lock` 中的应用自身版本
- `src-tauri/tauri.conf.json`
- 根 `package-lock.json` 中的应用自身版本
- `server/offline/docker-compose.yml` 中的版本镜像标签

同步脚本可重复运行，不更新第三方依赖。`npm run check-version` 只检查、不改文件；发现版本不一致时失败。`web-admin/package.json` 与它的锁文件仍独立维护，本次同样标记为 `2.0.0`。

`npm version <版本号>` 会触发同步和暂存相关文件，并通常创建 Git 提交和标签。希望手动提交时，编辑根 `package.json` 后单独运行同步和检查命令，不使用 `npm version`。

GitHub Release 由 `.github/workflows/release.yml` 统一编排：

1. 推送 `v2.0.0` 这样的版本标签，或手动运行 **Build & Release** 并填写已存在的标签。
2. 校验标签等于 `v` 加根版本，且检出的提交就是标签提交；未同步的版本文件会阻止发布。
3. 并行构建 Windows x64（NSIS / MSI / 便携 ZIP）、macOS arm64 / x64（DMG / 应用 ZIP）、Linux x64（deb / AppImage）。
4. 调用 `.github/workflows/docker.yml` 构建 Linux amd64 镜像，检查显式初始化、已有数据库启动和 Web 页面，再组装含部署脚本与模板的 `TEngineHttpV2.zip`。安装包不含真实配置或数据。
5. 所有构建成功后汇总附件，生成 SHA-256 校验文件并发布同一个 Release。任一构建失败时不执行发布，可修复后重新运行；普通分支推送不会发布。

Docker 工作流仅作为可复用构建调用，不再独立创建 Release。默认令牌仅有读取权限，最终发布任务单独获得 `contents: write`；不需要个人访问令牌。当前未配置 Windows 代码签名或 Apple Developer ID / 公证，macOS 使用 ad-hoc 签名。

提交并推送源码后，手动执行 `git tag v2.0.0` 与 `git push origin v2.0.0` 才会触发本次发布。仅配置工作流或暂存文件不会创建远端 Release。部署细节见 [README](../README.md#远程服务docker) 和 [离线安装说明](../server/offline/README.md)。

## Claude Code 迁移说明

- 新的开发入口为 `AGENTS.md` 与本指南。
- `docs/superpowers/specs/` 和 `docs/superpowers/plans/` 保留为历史档案，旧计划中的示例代码、命令和复选框不是当前执行指令。新文档放在 `docs/` 下与内容相符的位置。
- 旧计划原先强制调用 Superpowers 的入口已替换为档案说明，后续开发不依赖这些技能。
- `.claude/settings.local.json` 与 `.claude/worktrees/` 保留在本地并继续忽略。旧工作树可能包含独立提交或未提交内容，清理应作为单独任务处理。
- 此次迁移仅调整开发规范和文档入口，应用代码、Git 历史与发布流程保持原状。

多用户会话、数据库、项目内设计技能及浏览器验证命令见 [多用户工作台](multiuser.md)。

桌面隔离测试可以设置 `TENGINE_LOCAL_DATA_DIR`，配置将写入该目录的 config.json，不触及日常使用的本地配置。
