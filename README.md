# TEngine Http Server

基于 [Tauri 2](https://tauri.app/) + [Vue 3](https://vuejs.org/) + [Axum](https://github.com/tokio-rs/axum) 构建的桌面应用，为 Unity [TEngine](https://github.com/ALEXTANGXIAO/TEngine) 框架提供本地 HTTP 资源服务器，用于 [YooAsset](https://github.com/tuyoogame/YooAsset) 热更新资源的本地分发与测试。

替代传统的 `start.bat` + Nginx/Python 方案，提供可视化的 GUI 操作界面，一键启动、一键同步，开箱即用。

## 功能特性

- **本地 HTTP 静态文件服务器**：基于 Axum 实现高性能异步 HTTP 服务，支持目录浏览、MIME 自动识别、路径安全检查
- **资源版本同步**：自动扫描 YooAsset 构建产物，支持选择指定版本同步到服务目录，无需重启服务器即可热更新资源
- **多项目管理**：标签页式多项目支持，每个项目独立配置端口、Bundles 目录、包名和目标平台
- **多平台支持**：支持 Android、iOS、Windows、MacOS、Linux、WebGL 六大平台，可同时选择多个平台进行资源同步
- **CORS 跨域支持**：可选开启 CORS，WebGL 构建测试时必备
- **局域网访问**：自动检测本机局域网 IP，支持手机等设备通过局域网直接访问资源服务器
- **实时日志面板**：可视化展示所有 HTTP 请求日志和同步操作日志，支持按项目过滤、面板高度拖拽调整
- **配置自动持久化**：项目配置自动保存，下次启动恢复上次状态
- **跨平台桌面应用**：支持 Windows (.exe / .msi) 和 macOS (.app / .dmg)

## 技术栈

| 层级 | 技术 |
|------|------|
| 前端 | Vue 3 + TypeScript + Vite |
| 后端 | Rust + Tauri 2 |
| HTTP 服务 | Axum + Tower-HTTP |
| 异步运行时 | Tokio |

## 快速开始

### 环境要求

- [Node.js](https://nodejs.org/) >= 18
- [Rust](https://rustup.rs/) >= 1.70
- 系统依赖：
  - **Windows**：Visual Studio Build Tools（C++ 桌面开发工作负载）
  - **macOS**：Xcode Command Line Tools（`xcode-select --install`）

### 开发运行

```bash
# 安装前端依赖
npm install

# 启动开发模式（同时启动 Vite 和 Tauri）
npm run tauri dev
```

### 构建打包

```bash
# 构建生产版本
npm run tauri build
```

构建产物位于 `src-tauri/target/release/bundle/` 目录下。

> **注意**：Tauri 不支持交叉编译。需要在 Windows 上构建 `.exe`，在 macOS 上构建 `.app`。可通过 GitHub Actions 实现自动化双平台打包。

## 使用流程

1. **Unity 构建资源**：在 Unity 中通过 YooAsset 的 AssetBundle Builder 构建资源包
2. **配置项目**：在本软件中设置项目名称、Bundles 目录路径、端口、包名和目标平台
3. **同步资源**：点击「同步资源」将指定版本的 Bundle 文件同步到服务目录
4. **启动服务**：点击「启动服务」开启 HTTP 服务器
5. **Unity 接入**：在 Unity 项目中将 YooAsset 的 `HostPlayMode` 地址指向本地服务器（如 `http://192.168.x.x:8081/TEngine/Android/`）
6. **运行测试**：Play 运行游戏，在日志面板中实时观察资源请求情况

## 项目结构

```
TEngineHttp/
├── src/                    # 前端 (Vue 3)
│   ├── App.vue             # 主界面组件
│   ├── main.ts             # 入口文件
│   └── styles/             # 样式文件
├── src-tauri/              # 后端 (Rust + Tauri)
│   ├── src/
│   │   ├── lib.rs          # Tauri 命令定义与应用入口
│   │   ├── server.rs       # Axum HTTP 静态文件服务器
│   │   ├── sync.rs         # 资源版本同步逻辑
│   │   └── config.rs       # 项目配置管理与持久化
│   ├── Cargo.toml          # Rust 依赖
│   └── tauri.conf.json     # Tauri 应用配置
├── package.json            # Node.js 依赖
├── vite.config.ts          # Vite 配置
└── index.html              # HTML 入口
```

## License

MIT
