# 多用户资源工作台

## 首次部署

此版本使用全新数据目录，不导入旧 config.json、旧 JWT 或按项目名保存的远程资源。发现旧布局时启动失败并要求更换 DATA_DIR，不会删除旧数据。不要直接把原来的生产卷挂载给新系统。

Compose 默认使用新的 `tengine-management-data` 卷；原 `tengine-data` 卷保留。已有自行配置的挂载也必须改为独立目录或新卷。

设置 `BOOTSTRAP_ADMIN_USERNAME`、`BOOTSTRAP_ADMIN_PASSWORD` 和 `DATA_DIR`，启动时会创建数据库和第一个管理员。账号存在后初始化变量不再覆盖密码；首次登录必须改密。生产浏览器使用 HTTPS，`COOKIE_SECURE=true`（默认）；本机 HTTP 开发显式设置 `COOKIE_SECURE=false`。桌面远程连接也应使用 HTTPS。

只允许一个服务进程使用同一 DATA_DIR。SQLite 数据库为 `management.sqlite`，资源为 `resources/{project_id}/{project_version}/{platform}/_versions/{bundle}`，暂存上传为 `staging/`，重启清理中断上传。项目版本与平台设置作为项目配置 JSON 存入 SQLite，所有权、成员关系和修订号使用独立字段约束。

## 权限

管理员可管理所有项目、用户及全局设置。普通用户能创建项目，也能操作分享给自己的资源。只有项目所有者或管理员可添加/移除成员、转交项目或删除整个项目。

删除用户必须传入其全部项目的接收者映射；事务同时转交项目、删除成员关系与登录会话、删除账号。接收者必须启用且不能是被删除用户。至少保留一个有效管理员，不能删除自己。所有者转交后变为共享成员；删除用户流程会移除其成员关系。

## 两种反向代理部署

离线 ZIP 的安装包与实例目录已分离：安装包可放在 Downloads，`install.env` 选择固定实例目录，数据和实际配置保存在实例中。旧相对目录部署的迁移、更新备份及数据库保护见 [离线安装说明](../server/offline/README.md)。

浏览器不需要选择部署类型。直连、群晖反向代理和 Nginx 均使用相同接口；实时连接失败时客户端保持已有内容与表单，每 15 秒静默同步，连接按 2–30 秒逐步退避重试。实时连接恢复后自动恢复事件推送。此行为不替代代理自身的 WebSocket 配置。

### 群晖 DSM

在 DSM 7 的“控制面板 → 登录门户 → 高级 → 反向代理”中编辑 TEngine 对应规则，打开“自定义标题 / 自定义标头（Custom Header）”，从“新增 / 创建”下拉菜单选择 **WebSocket**，保存规则。DSM 6 的入口通常在“应用程序门户”。目标仍是 TEngine 容器的 HTTP 端口；浏览器入口使用 HTTPS，服务端保持 `COOKIE_SECURE=true`。

若普通页面和登录正常，但 WebSocket 返回 `Connection header did not include 'upgrade'`，说明升级请求头未传到后端，应检查这条规则。步骤依据 [Synology 官方说明](https://kb.synology.com/en-nz/Surveillance/tutorial/WebSocket_connection_fail_streaming_issue)。

### 服务器 Nginx

使用 [nginx.conf.example](../server/nginx.conf.example)，替换域名、证书路径与后端地址后放入 Nginx 的 `http` 上下文（通常为 `conf.d` 目录）。其中已包含 HTTP/1.1、Upgrade/Connection 转发、上传上限和长连接超时。示例假设 Nginx 在宿主机，后端通过 `docker-compose.prod.yml` 绑定 `127.0.0.1:8083`；Nginx 若也在容器内，后端地址改为共同 Docker 网络中的服务名。

先运行 `nginx -t`，通过后再执行受部署流程管理的 reload。协议升级头必须显式转发，见 [Nginx 官方 WebSocket 文档](https://nginx.org/en/docs/http/websocket.html)。服务端每 15 秒发送心跳，避免空闲日志连接被代理误判超时。

更新应用镜像后，重新创建应用容器并刷新浏览器；客户端修复不会自动改变 NAS 或服务器的代理设置。

## 登录与接口

`POST /api/auth/login` 接收 `{username,password,client:"web"|"desktop"}`。服务端 Argon2id 保存密码；Web 获得 HttpOnly/SameSite=Strict 会话 Cookie 和响应中的 csrf，桌面获得内存 Bearer token。`GET /api/auth/me` 恢复 Web 用户和 CSRF，Web 写请求必须带 `X-CSRF-Token`。会话默认 24 小时，改密、禁用、重置、删除及退出撤销对应会话。

- `PUT /api/auth/password`：`{current_password,password}`，成功后重新登录。
- `POST /api/auth/logout`：退出。
- `/api/admin/users`：GET 用户列表；POST `{username,password,role}` 创建。
- `/api/admin/users/:id`：PUT `{role,enabled}`；DELETE `{transfers:{project_id:recipient_user_id}}`。
- `PUT /api/admin/users/:id/password`：`{password}`，下次登录必须改密。
- `GET /api/users/lookup?username=...`：精确查找有效用户，仅返回 ID 和用户名。
- `POST /api/projects`：`{project_name,project_type,project_version,platforms,package_name}`，原子创建初始配置。类型缺省为 `bundle`，创建后固定；APK/普通文件的包名可省略，平台固定为 `Android`/`Any`。
- `GET /api/projects`：授权项目；GET `/api/projects/:id`：当前配置。
- 项目配置、成员、版本、发布及删除写请求需 `If-Match: <revision>`；冲突为 409。上传通过开始/提交时的修订与权限复查保证安全，不覆盖现有 Bundle。
- `/api/projects/:id/members`：GET 成员；POST `{user_id}`；DELETE `/members/:user_id` 移除。
- `PUT /api/projects/:id/owner`：`{user_id}` 转交。
- 原项目版本/上传/文件/清单 API 路径保留。activate 接收 `{}` 发布，或 `{unpublish:true}` 取消发布。删除正在发布的 Bundle 返回 409。
- `GET /api/audit?project_id=...&before=...`：按当前授权倒序读取，每页 100 条；管理员可读系统事件。
- `POST /api/auth/ws-ticket` 获取 30 秒一次性票据；`/api/ws/logs?ticket=...` 建立授权日志连接。权限变化通知客户端重新读取授权数据。
- `/api/settings/protocol-access` 仅管理员访问，控制资源下载协议，不关闭管理入口。

Bundle 公开下载路径为 `/res/{project_version}/{project_name}/{platform}/{file}`，例如 `/res/v1/RollCall/Android/DefaultPackage.version`。内部管理和磁盘存储仍使用 UUID，旧 UUID 链接仅兼容 Bundle，不移动已有数据。项目名称区分大小写，同类型内全局查重，不同类型可同名；旧配置缺少类型时按 Bundle 处理，同类型已有重名的名称路由仍返回 409。APK 使用 `/appversion/{project_version}/{project_name}/catalog.json` 及 `releases/{versionCode}/{apk}`，普通文件使用 `/file/{project_version}/{project_name}/{file}`，均省略平台层。改名会改变下载地址，下载开关仍有效。发布、APK 历史链接和迁移规则见 [APK 更新说明](apk-updates.md)。

`GET /api/resource-logs` 返回当前账号有权查看的最近资源请求；同一日志通过已有 WebSocket 推送。记录请求时间、项目、GET/HEAD、路径（不含查询参数）、实际 HTTP 状态码及结果。已知项目日志只对所有者、共享成员和管理员开放；不能归属项目的无效请求仅管理员可见。服务端保留最近 500 条于内存，重启清空，不向 SQLite 持续写入每个资源请求。前端在连接或重连时补取历史，Web 与桌面底部日志面板显示“资源”来源及 HTTP 状态，权限变化清除旧缓存。

底部默认显示“操作日志”（上传、发布、配置与客户端事件），“资源请求”为独立页签。两类各自保留最近 500 条，独立错误计数、清空和跟随滚动设置；资源请求不会挤占操作日志的缓存、触发其滚动或改变其错误数。切换分类支持左右方向键。服务端操作记录仍独立保存在 SQLite，不受资源请求内存上限影响。

成功日志表示服务端已读完响应资源，不表示游戏端确认收到或校验成功。响应头发送后的读失败、提前结束或流取消仍记录原 HTTP 状态（通常 200），并以失败/中断结果标记；发送前失败记录实际 403/404/409/500。未升级的旧服务器仍可连接，新客户端会提示其不支持资源请求日志。

## 客户端

Web 通过 URL hash 保存项目版本和平台，可直接打开与刷新。桌面本地与远程共用主题和基础控件，远程界面与 Web 共用 Workspace，传输分别使用浏览器 API 与 Tauri 原生插件。

桌面登录页保留“已保存的服务器”列表，可以新增、编辑、移除连接，保存连接名称、地址及用户名；登录时也会保存连接。可选的“记住密码”使用 macOS 钥匙串 / Windows 凭据库，不写入 localStorage；旧列表保留名称与地址，旧明文密码清除后需重新输入。读取受保护密码时按系统提示授权，更新未正式签名的应用后可能再次提示。改变连接地址或用户名会清空输入的密码，避免把旧服务器密码带到新目标。桌面会话仍仅在内存中，重启后点击登录即可使用保存的密码。

项目、项目版本与平台均可通过标签切换，新增版本及新增平台直接可见。新增平台应用到当前项目的所有项目版本，资源按版本和平台隔离。项目列表显示版本名称，整行可打开。

资源目录上传支持完整和增量两种方式。客户端按服务器、账号、项目记住 Bundles 目录。每次打开上传窗口会重新读取目录和服务器版本，自动选中本地最新构建，并以最近上传版本为基准（与是否已发布无关）；本地版本和对比基准均可手动切换。根据 MD5 和大小展示修改、新增、未变化和移除的文件，以及传输大小和节省量。移除项只说明两个版本的差异，不删除旧资源版本；已上传的同名版本会提示并禁止重复提交。一次上传最大 512MB。

默认手动发布。手动 / 自动发布偏好仅保存到当前客户端，并按服务器、账号、项目、项目版本、平台区分。自动模式在上传成功后再请求发布；发布失败时保留已上传资源。若上传期间其他成员切换了发布版本，自动发布停止并提示手动确认。资源版本删除为直接行操作，保留确认，已发布版本须先切换或取消发布。长文件名完整换行显示并可复制。

所有客户端输入框、文本域和可编辑内容统一关闭自动大写、自动纠正及拼写检查，包括动态打开的表单。

## 备份与恢复

停止服务后备份整个 DATA_DIR（含数据库、WAL/SHM 如存在、资源文件），再启动服务。恢复时停止服务，把整套备份放入独立目录后启动。数据库和资源必须成套恢复；不要只复制运行中的 SQLite 主文件。审计记录保留被删除用户的操作者快照。

## 设计与验证

技能固定在 `.agents/skills/`，版本见该目录 SOURCE.md。设计约定见 `.ui-craft/brief.md` 和 `.ui-craft/tokens.md`；运行时主题在 `shared/theme.css`。系统默认深色，可切浅色；面向 960×700、800×600 窗口。

按 `docs/development.md` 运行构建和测试。浏览器测试从仓库根执行：

```bash
npm --prefix web-admin exec playwright install chromium
npm --prefix web-admin run build
npm --prefix web-admin run test:e2e
```

先按开发指南安装根目录和 `web-admin/` 的依赖。测试自动启动独立的 18092 端口服务和 1431 端口桌面前端，并把随机数据目录、截图和失败跟踪放在 Cache/tests，完全隔离生产资源。桌面交互回归使用受控 Tauri IPC 响应模拟延迟与项目切换；这部分不能代替 Tauri 原生运行验证。

本次执行的结果与边界见 [改造验证记录](multiuser-validation.md)。

实时连接、弹窗及并发操作修复见 [交互稳定性修复](ui-stability.md)。
