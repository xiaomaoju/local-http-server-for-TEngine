# APK 更新协议与三种下载命名空间

项目创建时选择 `bundle`、`apk` 或 `files`，创建后类型固定。名称区分大小写，只在同类型内查重；管理接口、权限和文件目录始终按独立 UUID 隔离。同名的三个项目可分别由不同账号管理。

## 公开下载

| 类型 | 地址 |
| --- | --- |
| Bundle | `/res/{项目版本}/{项目名}/{平台}/{文件名}` |
| APK 当前 catalog | `/appversion/{项目版本}/{项目名}/catalog.json` |
| 已发布 APK | `/appversion/{项目版本}/{项目名}/releases/{versionCode}/{APK文件名}` |
| 独立教程附件 | `/appversion/{项目版本}/{项目名}/docs/{文件名}` |
| 普通文件 | `/file/{项目版本}/{项目名}/{文件名}` |

两个新模式的公开地址不带平台层。内部管理接口与存储仍使用固定 `Android`（APK）、`Any`（普通文件）分区。Bundle 的旧 UUID 下载地址继续兼容，只解析 Bundle 类型。旧项目缺少 `project_type` 时视为 Bundle，数据库升级只增加类型/名称查询索引，不移动已有资源、重置账号或强制改名。同类型已有重名的名称路由返回 409，不任选其中一个。

项目或项目版本改名会改变公开地址，需要同步更新客户端配置。目录地址是下载地址的前缀，不提供公开文件列表。三个命名空间均支持 GET/HEAD、协议/下载开关和有权限过滤的资源请求日志。APK 和普通文件只支持平铺文件，不支持子目录与断点续传。

## catalog.json v1

catalog 由应用构建发布流程生成，上传后原样存储。服务端检查文件组合和发布编号，不解析 JSON、不修改字段、不验证 APK 的实际包名或签名。上传方必须保证以下协议，应用端检查字段与 APK 一致性。

| 字段 | 类型 | 要求 |
| --- | --- | --- |
| `schemaVersion` | integer | 固定 `1` |
| `packageName` | string | 与实际 APK 的 Android 包名一致 |
| `versionCode` | integer | 正整数；发布编号使用它的十进制字符串，不带前导零 |
| `versionName` | string | 显示版本号 |
| `minSdk` | integer | 实际 APK 的最低 Android API 等级 |
| `changelog` | string | 更新说明，可包含换行 |
| `apkPath` | string | 相对于 catalog 所在目录的 `releases/{versionCode}/{APK文件名}` 路径；文件名按 URL 路径段编码 |
| `apkSize` | integer | 实际 APK 的文件大小，单位字节 |
| `sha256` | string | 实际 APK 的 SHA-256，64 位小写十六进制字符串 |

不把 TEngineHttp 的 `package_name`（YooAsset 资源包名）当作 Android 包名。APK 项目不要求 YooAsset 包名。

## ZFlip5 发布步骤

1. 沿用 ZFlip5 项目的包名 `io.github.flipcover.controls` 和签名密钥，构建递增 `versionCode` 的 APK。
2. 从该 APK 取得版本信息、最低 API 等级、字节数和 SHA-256，生成 `catalog.json`；`apkPath` 必须包含本次发布编号。
3. 在 TEngineHttp 创建 `ZFlip5`、类型“APP更新”（项目标签标记为 Ⓐ）、项目版本 `v1`；选择 APK 和 catalog，填写发布编号并完整上传。
4. 确认上传成功后发布。应用端固定读取 catalog，再解析相对路径下载安装包，并核验大小、SHA-256、包名、签名与版本后交给系统确认安装。

例如发布编号 `69`、安装包 `flipcover-0.18.11.apk`：

```text
catalog 地址：/appversion/v1/ZFlip5/catalog.json
catalog.apkPath：releases/69/flipcover-0.18.11.apk
APK 地址：/appversion/v1/ZFlip5/releases/69/flipcover-0.18.11.apk
```

应用端更新功能由 ZFlip5 项目实现。TEngineHttp 的托管协议不要求 Unity 或 YooAsset。

## 发布、取消及删除

上传先进入暂存区，完整保存两个文件后形成待发布版本；失败不改变当前发布。catalog 使用固定地址与 `Cache-Control: no-store`，发布原子切换当前版本指针。历史已发布 APK 地址继续有效；读到旧 catalog 的客户端在新版发布后仍可下载旧 APK。

只有当前或历史已发布的 APK 可从版本化路径下载。曾发布的编号不能覆写或复用，即使安装包已被删除，历史编号仍保留在项目元数据中。不能删除当前发布版本；删除历史版本会使对应旧链接返回 404。取消发布让 catalog 返回 404，历史 APK 继续可下载；关闭客户端下载让 catalog 和历史 APK 均返回 403。

普通文件按完整批次形成版本，发布后固定文件地址指向当前发布版本；上传同名版本被拒绝，允许切换旧版本。取消发布停止该项目版本的全部普通文件下载。

## 教程附件

所有 APP 更新项目均可独立管理教程附件：同一项目版本共用一套文件，不绑定 versionCode，也不要求先上传 APK。支持任意名称的单个 HTML；自包含 HTML 无需额外文件。若 HTML 使用相对路径引用其他页面、CSS、JS 或图片，应一起上传这些平铺文件。服务端不改写 HTML、不执行服务端脚本，不读取外部文件或自动补齐依赖。

桌面提供“选择目录 / 选择文件”。选择任意平铺目录后，窗口会显示待上传文件与覆盖项；隐藏文件跳过，子目录及符号链接拒绝。远程按服务器、账号、项目 UUID 和项目版本记忆目录，本地按项目 UUID 和项目版本记忆；再次打开自动读取，支持手动“重新读取”。上传前再次检查目录清单，文件名或大小变化时要求重新确认。浏览器可以选择文件或目录，但无法自动恢复本机目录访问权限；文件夹上传只保留文件名，不包含所选根目录名。

确认上传后立即生效：同名替换，未选择的文件保留；目录中消失的文件不会自动删除服务器文件。删除是单独的确认操作。上传失败、并发冲突或配置/审计保存失败时，不切换已生效附件。每套附件最多 512MB；远程单次请求最多 512MB（含 multipart 开销），本地单批文件最多 512MB。APK、catalog.json、路径穿越及大小写重名均拒绝。

管理接口仅接受有项目访问权限的成员：

| 方法 | 管理路径 | 行为 |
| --- | --- | --- |
| GET | `/api/projects/{id}/project-versions/{pver}/attachments` | 返回 `files`、`snapshot` 和最新 `project` |
| POST | 同上 | multipart `files` 批量合并上传；需 `If-Match` |
| DELETE | 上述路径加 `/{文件名}` | 删除单个附件；需 `If-Match` |

Web 写入继续校验 CSRF，桌面使用已有 Bearer 会话；并发修改返回 409，保留客户端文件选择，重新读取后再确认。响应中的文件项沿用 `name`、`size`、`modified_timestamp`。所有文件名作为单独 URL 路径段编码。

例如 `/appversion/v1/MyApp/docs/guide.html` 始终访问当前附件。GET/HEAD 均受现有协议及客户端下载开关控制，关闭返回 403，缺失返回 404。HTML 在浏览器展示，CSS/JS/图片/字体/媒体按类型提供，其他附件默认下载；均使用 `Cache-Control: no-store`。取消发布或删除 APK 不影响教程；删除项目或项目版本时一并清理，项目或项目版本改名会改变公开地址。

同域名的活动文档使用 CSP 沙箱，允许内嵌和同站脚本、样式、图片、关联 iframe、受沙箱约束的新窗口及下载。禁止继承管理站点来源权限、网络 API 请求、表单提交、插件和自定义 base URL；localStorage、Cookie 及服务工作线程不可作为教程持久化机制。需要存储的 HTML 应自行处理不可用情况。管理界面的“打开网页”使用外部浏览器，不嵌入执行上传内容。

附件快照保存于资源根目录下 `{项目UUID}/{项目版本}/_attachments/{快照UUID}/`，本地沿用 `{项目UUID}/local/_attachments/{快照UUID}/`。远程 `ProjectVersion`、本地项目配置新增可选 `attachment_snapshot`，旧配置缺失时视为无附件；不修改 catalog 协议、SQL 表或原 APK 目录。完整新快照落盘后切换配置指针，远程指针与操作日志在同一事务保存；旧快照等待在途读取结束后清理，启动清理孤立快照。不提供附件历史回滚。备份需同时保留配置/数据库及资源目录。

## 桌面本地与代理

桌面 APK 模式支持“文件上传 / 本地构建目录”。构建根目录由用户选择，名称任意；可以直接包含 catalog 与 APK，也可以在一级版本子目录内各保存一对。应用读取 catalog.versionCode，按数值从高到低选择最新版本，并自动填写发布编号。apkPath 使用公开路径 `releases/{versionCode}/{文件名}`；本地 APK 从对应 catalog 的同目录按文件名配对。根目录不会写死为 `update-release`、`release` 或 `dist`。

远程目录记忆按服务器、账号、项目 UUID 和项目版本隔离；本地按项目 UUID 和项目版本隔离，重启后恢复。选择目录后可“重新读取”，也可在项目页直接“上传最新版本 / 上传并发布最新版本”；本地提供“导入最新版本”。每次打开或一键操作均重新扫描，已有或曾发布编号不能覆写。最新编号目录缺少 catalog、文件缺失、协议无效或编号重复时提示错误；上传前固定文件快照并检查大小、SHA-256，catalog 原始字节保持不变，完成或失败后清理临时快照。手动文件选择和 Web 文件上传继续可用。

本地 APK/普通文件通过桌面选择并导入，保存到应用配置目录的 `resources/{项目UUID}/local/{Android|Any}/_versions/{发布版本}`。源文件删除不影响导入内容。当前发布和 APK 历史编号在本地配置中持久化；运行中的服务切换版本或下载开关无需重启。Bundle 保留原来的构建目录和同步布局，并提供 `/res` 前缀及原路径两种地址。

远程请求上限为 512MB，包含 multipart 请求开销；桌面本地导入的文件内容上限为 512MB。没有自动生成 catalog、断点续传、APK 内容解析或安装功能。

使用 Nginx/群晖时，将 `/api`、`/res`、`/appversion`、`/file` 原样转发到同一个后端。仓库的 Nginx 示例通过 `location /` 覆盖这些前缀，不需要额外路径重写。Web 开发代理也覆盖全部下载命名空间。
