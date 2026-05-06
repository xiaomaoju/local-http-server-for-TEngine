# 项目版本与平台访问开关设计

**日期**：2026-05-06
**作者**：quyongle
**状态**：设计中

## 背景

当前 TEngineHttp 服务在远程模式下，资源访问 URL 为：

```
/res/{project_name}/{platform}/{file}
```

URL 中不携带版本信息，依赖"激活"操作把某个版本的文件复制到平台根目录。这种设计有两个限制：

1. 无法同时对外提供多个版本（例如同时支持老 SDK 和新 SDK）
2. 没有访问控制能力，无法临时下线某个平台的资源

本设计引入 **两层版本模型**（项目版本 / Bundle 版本）和 **平台访问开关**，解决这两个问题。

> 不考虑现有数据迁移。视为全新部署，旧 URL 与旧目录结构直接弃用。

## 目标

1. 在远程模式资源 URL 中引入"项目版本"段，客户端 SDK 可以稳定地按 `/res/v1/...`、`/res/v2/...` 访问
2. 同一项目下可并存多个项目版本，每个项目版本独立管理 bundle 与激活状态
3. 每个（项目版本 × 平台）有独立的访问开关，关闭后对应资源 URL 返回 403
4. 本地模式（Tauri）支持平台访问开关（不引入项目版本概念）
5. UI 紧凑、可折叠，避免无谓增高/增宽窗口

## 概念模型

### 两层版本模型

```
Project (项目)
└── ProjectVersion (项目版本，v1/v2/...)
    └── Platform (Android/iOS/...)
        ├── access_enabled    平台访问开关
        ├── active_bundle     激活的 bundle 版本（仅元数据）
        └── BundleVersions[]  实际上传的资源版本
```

| 概念 | 含义 | 出现在 URL 中 |
|------|------|---------------|
| 项目（Project） | 顶层容器，对应客户端工程 | 是（`project_name` 段） |
| 项目版本（ProjectVersion） | 资源访问通道，稳定标识 | 是（`v1`/`v2` 段） |
| Bundle 版本（BundleVersion） | 实际上传的资源版本，可迭代 | 否（仅服务端管理） |
| 激活（Active） | 项目版本 × 平台下当前对外提供哪个 bundle | 否（仅元数据） |

### 关键性质

- **项目版本独立**：v1 与 v2 的 bundle 完全隔离，互不共享
- **激活仅元数据**：不再复制文件到根目录
- **平台开关粒度**：每个 (project, project_version, platform) 一个独立开关

## 配置结构（server/config.json）

```json
{
  "projects": [
    {
      "id": "uuid",
      "project_name": "MyProject",
      "package_name": "DefaultPackage",
      "platforms": ["Android", "iOS"],
      "project_versions": [
        {
          "name": "v1",
          "platform_settings": {
            "Android": { "access_enabled": true,  "active_bundle": "1.0.1" },
            "iOS":     { "access_enabled": false, "active_bundle": "1.0.0" }
          }
        },
        {
          "name": "v2",
          "platform_settings": {
            "Android": { "access_enabled": true,  "active_bundle": "2.0.0" }
          }
        }
      ]
    }
  ]
}
```

**与现状的差异**

- 删除 `active_versions: HashMap<String, String>`
- 新增 `project_versions: Vec<ProjectVersion>`

## 磁盘存储布局

```
resources/
└── MyProject/
    ├── v1/
    │   ├── Android/
    │   │   └── _versions/
    │   │       ├── 1.0.0/
    │   │       └── 1.0.1/
    │   └── iOS/
    │       └── _versions/
    │           └── 1.0.0/
    └── v2/
        └── Android/
            └── _versions/
                └── 2.0.0/
```

- 路径相对现状增加 `<project_version>/` 段
- 不再有"激活版本副本"目录
- 删除 ProjectVersion → 删除 `resources/<project>/<project_version>/` 整棵目录

## URL 路由

### 资源访问 URL

```
GET /res/{project_version}/{project_name}/{platform}/{file_path}
```

### 解析与响应流程

```
GET /res/v1/MyProject/Android/file.bundle
   1. 查 projects[project_name=MyProject]
      不存在 → 404
   2. 查 project_versions[name=v1]
      不存在 → 404
   3. 检查 platform_settings[Android].access_enabled
      false → 403 Forbidden
   4. 读 platform_settings[Android].active_bundle
      空 → 404（无激活版本）
   5. 服务 resources/MyProject/v1/Android/_versions/{active_bundle}/file.bundle
      文件不存在 → 404
```

### 旧 URL

`/res/{project_name}/{platform}/{file}` 直接 404，不做兼容。

## 服务端 API（server/src/api.rs）

### 新增 API

```
POST   /api/projects/:id/project-versions
       创建项目版本
       Body: { "name": "v1" }

GET    /api/projects/:id/project-versions
       列出项目版本

DELETE /api/projects/:id/project-versions/:pver
       删除项目版本（连带其下所有 bundle 与磁盘目录）

PUT    /api/projects/:id/project-versions/:pver/platforms/:plat/access
       切换平台访问开关
       Body: { "enabled": true | false }
```

### 改造 API（路径加 :pver 段）

| 现状 | 新 |
|------|----|
| `/api/projects/:id/upload` | `/api/projects/:id/project-versions/:pver/upload` |
| `/api/projects/:id/incremental-upload` | `/api/projects/:id/project-versions/:pver/incremental-upload` |
| `/api/projects/:id/manifest` | `/api/projects/:id/project-versions/:pver/manifest` |
| `/api/projects/:id/versions` | `/api/projects/:id/project-versions/:pver/versions` |
| `/api/projects/:id/versions/:ver/activate` | `/api/projects/:id/project-versions/:pver/versions/:ver/activate` |
| `/api/projects/:id/versions/:ver` (DELETE) | `/api/projects/:id/project-versions/:pver/versions/:ver` |
| `/api/projects/:id/files` | `/api/projects/:id/project-versions/:pver/files` |
| `/api/projects/:id/status` | `/api/projects/:id/project-versions/:pver/status` |

### 行为变化

- **激活 API**：仅更新 `active_bundle` 元数据；不再复制文件到平台根目录
- **删除 ProjectVersion**：递归删除磁盘目录与所有 bundle 元数据
- **平台开关**：写入配置后立即生效，资源访问路由读取最新配置

## 服务端模块改动概要

### `server/src/config.rs`

```rust
pub struct PlatformSettings {
    pub access_enabled: bool,
    pub active_bundle: Option<String>,
}

pub struct ProjectVersion {
    pub name: String,
    pub platform_settings: HashMap<String, PlatformSettings>,
}

pub struct ProjectConfig {
    pub id: String,
    pub project_name: String,
    pub platforms: Vec<String>,
    pub package_name: String,
    pub project_versions: Vec<ProjectVersion>,
    // 删除 active_versions
}
```

### `server/src/storage.rs`

- 所有路径方法多一个 `project_version: &str` 参数
- 删除 `activate_version`（不再复制）；新增 / 修改方法以反映项目版本层
- `delete_project_version(project_name, project_version)` 删除整棵 v 目录

### `server/src/serve.rs`

- 解析 URL：第一段为 `project_version`，第二段为 `project_name`
- 按上面"解析与响应流程"逐步检查并响应

### `server/src/api.rs`

- 新增 4 条 API 路由
- 8 条现有 API 路径前缀修改，处理函数加 `project_version` 参数

## 客户端（src/）改动

### `src/api/remote.ts`

- 所有方法签名加 `projectVersion: string` 参数
- 新增方法：`createProjectVersion`、`listProjectVersions`、`deleteProjectVersion`、`setPlatformAccess`

### `src/components/RemoteMode.vue`

布局保持紧凑、可折叠：

```
┌─────────────────────────────────────────────────────┐
│ [项目1] [项目2] [+]                  ← 一级标签       │
├─────────────────────────────────────────────────────┤
│ [v1] [v2] [v3] [+]                   ← 二级标签       │
├─────────────────────────────────────────────────────┤
│ ▼ 项目设置 (折叠)                                    │
│   项目名 / 包名 / 平台多选                            │
├─────────────────────────────────────────────────────┤
│ 平台访问  Android●  iOS○  WebGL●  (内联开关行)       │
├─────────────────────────────────────────────────────┤
│ 同步平台 [Android ▼]   [▶ 同步资源]   当前激活: 1.0.1 │
├─────────────────────────────────────────────────────┤
│ ▼ Bundle 版本列表                                     │
│   1.0.1  当前  [浏览] [激活] [删除]                  │
│   1.0.0       [浏览] [激活] [删除]                   │
├─────────────────────────────────────────────────────┤
│ ▶ 日志                                               │
└─────────────────────────────────────────────────────┘
```

UI 设计原则：

- **二级标签**复用一级标签样式但缩小尺寸/降低对比度
- **项目设置**默认折叠，点击标题展开
- **平台访问开关**用紧凑的内联开关行展示，每个平台一个小型 toggle
- **Bundle 版本列表**默认展开，是高频操作区域
- **日志**默认折叠到底部 36px，点击展开
- 整体保持当前窗口宽高，**新增内容靠折叠/紧凑布局承载**

### `src/components/LocalMode.vue`

- 不引入项目版本概念
- 在配置区域内联一行平台访问开关：每个已选平台一个 toggle，紧贴在平台多选标签旁边
- 持久化到本地 `config.json`

### Tauri 后端（`src-tauri/src/server.rs`、`src-tauri/src/config.rs`）

- `ProjectConfig` 增加 `platform_access: HashMap<String, bool>` 字段
- 服务文件请求时，从 URL 解析平台段（首段），查表判断；关闭则 403

## 状态管理与并发

- 平台开关切换通过 `RwLock<AppConfig>` 写锁保护
- 资源路由用读锁查询配置，单次请求快速完成，无瓶颈

## 边界与异常

| 场景 | 行为 |
|------|------|
| 项目版本名重复 | API 返回 409 Conflict |
| 项目版本名包含 `/`、`\`、`..`、空字符串 | API 返回 400 |
| 删除项目版本时该版本下有 bundle | 一并删除（前端 UI 加二次确认弹窗） |
| 上传 bundle 时项目版本不存在 | API 返回 404 |
| 激活的 bundle 在磁盘上被外部删除 | 资源路由返回 404 |
| 平台未在 `platforms` 列表中 | 资源路由返回 404 |

## 测试策略

- **单元测试**（storage.rs）：项目版本 CRUD、bundle 隔离、删除级联
- **集成测试**（api.rs）：新 API 路由、平台开关切换、资源访问 403/404 路径
- **端到端**（手工）：UI 上创建 v1/v2 → 各传不同 bundle → 激活后通过新 URL 访问

## 实施范围

### 范围内
- 新数据模型与配置序列化
- 资源 URL 路由改造
- 服务端 API 改造与新增
- 客户端 RemoteMode.vue 与 LocalMode.vue 的 UI 改造
- Tauri 本地服务器的平台开关

### 范围外
- 数据迁移（用户明确不考虑旧数据）
- 项目版本重命名（v1 改成 v1-prod 之类）
- 项目版本之间共享 bundle
- 客户端 SDK 改造（由 SDK 团队另行处理）
