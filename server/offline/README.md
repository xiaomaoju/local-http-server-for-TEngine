# TEngineHttpV2 离线安装

安装包和运行实例是两个独立目录。安装包可以删除、重新解压或更换位置；实例目录保存配置、账号、资源和备份，不能随安装包一起删除。

```text
/volume3/NAS文件夹/Downloads/TEngineHttpV2/  安装包
  deploy.sh / batch-deploy.sh
  install.env                  选择实例目录
  settings.env.example         仅供首次安装的配置模板
  tengine-server.tar           已构建的 linux/amd64 镜像

/volume3/docker/TEngineHttpV2/             运行实例
  docker-compose.yml
  settings.env                 当前实例的真实配置
  data/management.sqlite       用户与项目数据库
  data/resources/              资源
  backups/                     更新前的完整数据和配置备份
```

## 安装和更新

确认安装包的 `install.env`：

```dotenv
INSTANCE_DIR=/volume3/docker/TEngineHttpV2
```

可以改成其他绝对路径，例如普通 Linux 服务器的 `/opt/TEngineHttpV2`。路径末级保持 `TEngineHttpV2`，并位于安装包之外。首次安装前可编辑 `settings.env.example`：默认管理员 `children`，服务端口 `PORT=8083`，密码留空时自动生成。

在安装包目录运行：

```sh
sudo sh deploy.sh
```

首次安装会在实例目录创建配置，单独执行数据库初始化命令，再启动服务。账号密码保存在实例目录的 `settings.env`。运行容器始终设置 `REQUIRE_EXISTING_DB=true`：数据库缺失、用户表不存在或没有用户都会拒绝启动，不能用初始化变量静默重建账号。

已有实例会提示 `Y=覆盖，N=忽略`，默认 N。Y 更新前先停服备份整个数据目录和配置，再替换程序。已有实例的 `settings.env` 不会被包内模板覆盖。修改实例端口等配置后重新执行安装包中的 `deploy.sh`。

端口只配置一个 `PORT`：填写 `8083` 时，宿主机映射、容器监听和健康检查全部使用 `8083`；改为其他端口时一起变化，无需编辑 Compose。旧版 `HOST_PORT` 在备份后自动转换为 `PORT` 并保留原值；若已手动填写 `PORT`，则使用 `PORT` 并移除旧键。反向代理的目的地端口须填写同一值，脚本不会修改 DSM / Nginx 的配置。

旧版把数据放在解压目录时，先将新包解压到**另一个安装目录**，保留原目录，再执行脚本。脚本根据旧容器的 `/data` 挂载找到数据，停服备份后复制到独立实例目录，并保留原目录。目标目录已有另一份数据库时停止，不会自动合并或覆盖。只有旧数据仍在时才能迁移，脚本不能恢复已经删除的数据库。

如果旧测试安装目录已被删除，但容器还在，脚本会显示原路径和新的实例路径，并询问 `确认放弃旧数据并重新安装？Y/N`，默认 N。只有原挂载目录不存在或完全为空、目标实例没有配置和数据时才允许此操作。确认 Y 后先校验镜像并初始化新数据库，成功后移除残留容器并启动新实例；不删除旧目录或 Docker 卷。新账号使用首次安装模板，已丢失的数据无法恢复。普通更新的 `--yes` 不会跳过这个单独确认，非交互运行默认取消。

已有实例的配置仍在、目录中还有资源或数据库残留时，缺库仍会停止，不会重新初始化。首次初始化失败也应先检查日志和实例目录，不把更新当作重置账号的方式。

## 群晖与 Nginx

群晖 DSM 反向代理：HTTPS 域名入口 → HTTP `127.0.0.1:8083`，在自定义标头中创建 **WebSocket**，绑定有效证书。

普通服务器：使用包内 `nginx.conf.example`，替换域名、证书和后端地址，`nginx -t` 通过后重载。默认后端端口为 8083，与实例的 `PORT` 保持一致。代理位于另一容器时使用共同 Docker 网络中的后端地址。

两种方式都保持 `COOKIE_SECURE=true`；仅隔离的本机 HTTP 测试使用 false。

## 批量发布

`servers.txt` 第三列填写**远程安装包目录**，例如 `/volume3/NAS文件夹/Downloads/TEngineHttpV2`。当前清单格式支持中文，不接受空格和单引号；也可使用 `/opt/packages/TEngineHttpV2` 等英文字母路径。

在发布电脑（macOS / Linux）运行 `bash batch-deploy.sh`。已有目标在上传前询问 Y/N。每台主机的 `install.env` 只在首次上传时创建；以后保留其配置。上传不包含运行实例的 `settings.env` 和 `data/`，初始化模板只对新实例生效。已确认 Y 时由批量脚本传递 `--yes`，不重复询问。

## 备份与恢复

更新备份位于实例的 `backups/日期时间-进程号/`：`data.tar` 包含整个 SQLite 数据库、可能存在的 WAL/SHM、资源及元数据，另附 `settings.env` 和原 Compose 配置。备份在停服后生成，不自动删除。

恢复前停止实例，另存当前 data 和配置，再恢复选定备份的整个 data 及配套配置；不能只替换运行中的 SQLite 主文件。旧迁移源也会保留。备份空间不足会中止更新；Compose 替换旧容器前失败时，会尝试恢复原运行容器。

## 源码与打包

本目录是部署脚本和模板的源码。交付包沿用 `Cache/deploy/TEngineHttpV2.zip`，顶层目录 `TEngineHttpV2`；只放本目录程序文件、镜像归档、校验与构建信息、代理示例及说明。**不打包 `data/`、实际 `settings.env`、数据库或备份。** 包中的 `README-部署说明.md` 来自本文件。

推送与根版本一致的 `v*` 标签后，GitHub **Build & Release** 自动构建并将 `TEngineHttpV2.zip` 附加到 Release。镜像标签为 `tengine-server:{版本}-amd64`，由根目录的 `npm run sync-version` 同步到本目录 Compose 文件；安装脚本从该文件读取标签。批量部署只要求正式包内文件，不依赖某次本地修复说明。

镜像首次初始化命令为 `tengine-server --initialize`，遇到已有数据库会拒绝运行。日常启动不传此参数。镜像健康检查自动读取 `PORT`。

Bundle 下载目录采用 `/res/v1/RollCall/Android/`；APK 使用 `/appversion/v1/RollCall/catalog.json` 与 `releases/{versionCode}/{apk}`，普通文件使用 `/file/v1/RollCall/{file}`，后两种省略平台层。名称在同类型内查重，不同类型可同名。旧 UUID 地址仅兼容 Bundle，旧配置缺少类型时视为 Bundle，磁盘资源不搬迁。同类型已有重名保留，但名称下载地址返回 409；改名会改变名称下载地址。数据库升级增加类型/名称查询索引，不重置用户、项目或资源。详见 [APK 更新说明](../../docs/apk-updates.md)。所有下载前缀需原样转发到后端，包内 Nginx 的 `location /` 已覆盖。

Web 和新版桌面客户端底部日志支持资源访问路径、HTTP 状态及失败原因；普通用户仅能查看有权限项目，未知路径只对管理员显示。服务端只在内存保留最近 500 条资源请求，重启清空。实时推送仍需代理正确转发 WebSocket，客户端断线时通过已认证接口定时补取最近记录。

“操作日志”与“资源请求”为两个独立页签，默认操作日志。资源刷屏不会挤掉正常操作记录，两边分别清空、计数和跟随最新。
