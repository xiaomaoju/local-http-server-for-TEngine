#!/bin/sh
# Linux amd64 / Synology: sudo sh deploy.sh
set -eu
package_dir=$(CDPATH= cd "$(dirname "$0")" && pwd -P)
cd "$package_dir"
overwrite_confirmed=false
case "$#:${1:-}" in
  0:) ;;
  1:--yes) overwrite_confirmed=true ;;
  *) echo '用法：sh deploy.sh [--yes]'; exit 1 ;;
esac
fail() { echo "错误：$*" >&2; exit 1; }
[ "$(uname -s)" = Linux ] && [ "$(uname -m)" = x86_64 ] || fail '请在 Linux x86_64 / 群晖上运行。'
command -v docker >/dev/null 2>&1 || fail '请安装 Docker（群晖为 Container Manager）。'
docker info >/dev/null 2>&1 || fail '无法连接 Docker，请启动服务并使用 sudo。'

compose_kind=''
compose_path=''
if docker compose version >/dev/null 2>&1; then
  compose_kind=plugin
elif command -v docker-compose >/dev/null 2>&1; then
  compose_path=$(command -v docker-compose)
elif [ -x /var/packages/ContainerManager/target/usr/bin/docker-compose ]; then
  compose_path=/var/packages/ContainerManager/target/usr/bin/docker-compose
else
  fail '未找到 Docker Compose。'
fi
[ -f install.env ] || fail '缺少 install.env。'
# Read a literal path; never execute configuration as shell code.
instance_dir=$(sed -n 's/^INSTANCE_DIR=//p' install.env | tr -d '\r' | tail -n 1)
case "$instance_dir" in
  \"*\") instance_dir=${instance_dir#\"}; instance_dir=${instance_dir%\"} ;;
  \'*\') instance_dir=${instance_dir#\'}; instance_dir=${instance_dir%\'} ;;
esac
instance_dir=${instance_dir%/}
case "$instance_dir" in /*/TEngineHttpV2) ;; *) fail 'INSTANCE_DIR 必须是以 /TEngineHttpV2 结尾的绝对路径。' ;; esac
case "$instance_dir/" in */../*|*/./*) fail 'INSTANCE_DIR 不能包含相对跳转。' ;; esac
if [ -d "$instance_dir" ]; then instance_dir=$(cd "$instance_dir" && pwd -P); fi
case "$instance_dir/" in "$package_dir/"*) fail '实例目录必须与安装包目录分开，不能放在安装包里面。' ;; esac
case "$package_dir/" in "$instance_dir/"*) fail '请将安装包放在实例目录之外。' ;; esac

data_dir="$instance_dir/data"
source_data="$data_dir"
source_settings="$instance_dir/settings.env"
mode=fresh
recreate_missing=false
missing_source=''
existing_container=$(docker ps -aq --filter 'name=^/tengine-http-multiuser$') || fail '无法检查已有容器。'
was_running=false
if [ -n "$existing_container" ]; then
  mount_type=$(docker inspect --format '{{range .Mounts}}{{if eq .Destination "/data"}}{{.Type}}{{end}}{{end}}' "$existing_container")
  [ "$mount_type" = bind ] || fail '现有容器使用非目录挂载，请先安排数据迁移，不会自动切换数据库。'
  source_data=$(docker inspect --format '{{range .Mounts}}{{if eq .Destination "/data"}}{{.Source}}{{end}}{{end}}' "$existing_container")
  source_empty=false
  if [ -d "$source_data" ]; then
    source_entries=$(ls -A "$source_data") || fail '无法读取原数据目录，请检查目录权限。'
    if [ -z "$source_entries" ]; then source_empty=true; fi
  fi
  if [ ! -e "$source_data" ] || [ "$source_empty" = true ]; then
    # A deleted test installation may leave a container behind. Never replace another instance.
    [ ! -e "$instance_dir/settings.env" ] && [ ! -e "$instance_dir/.tengine-instance" ] || fail '旧容器的数据已丢失，但目标实例已有配置。请检查实例目录或备份，不会重新初始化。'
    recreate_missing=true
    missing_source="$source_data"
    source_data="$data_dir"
    source_settings="$instance_dir/settings.env"
  else
    [ -d "$source_data" ] || fail '现有容器的数据挂载不是目录，请检查挂载路径。'
    source_data=$(cd "$source_data" && pwd -P)
    source_settings="$(dirname "$source_data")/settings.env"
    was_running=$(docker inspect --format '{{.State.Running}}' "$existing_container")
    mode=update
    if [ "$source_data" != "$data_dir" ]; then mode=migrate; fi
  fi
elif [ -f "$data_dir/management.sqlite" ]; then
  mode=update
elif [ -e "$instance_dir/settings.env" ] || [ -e "$instance_dir/.tengine-instance" ]; then
  fail '已有实例缺少数据库，拒绝自动初始化。请检查实例目录或备份。'
fi
if [ "$mode" != fresh ]; then
  [ -s "$source_data/management.sqlite" ] || fail '已有部署的数据库缺失或为空，拒绝覆盖及重新初始化。'
  [ -f "$source_settings" ] || fail "缺少原实例配置：$source_settings，请先恢复配置。"
fi
if [ "$mode" = migrate ]; then
  [ ! -e "$data_dir/management.sqlite" ] || fail '原挂载与目标实例都存在数据库，拒绝自动覆盖。请先确认要使用哪份数据。'
fi
if [ "$mode" = fresh ] || [ "$mode" = migrate ]; then
  if [ -d "$data_dir" ]; then
    target_entries=$(ls -A "$data_dir") || fail '无法读取目标 data 目录，请检查目录权限。'
    [ -z "$target_entries" ] || fail '目标 data 目录不是空目录，拒绝初始化或合并覆盖。'
  fi
fi

echo "安装包目录：$package_dir"
echo "实例目录：$instance_dir"
if [ "$recreate_missing" = true ]; then
  echo "残留容器的原数据目录已丢失或为空：$missing_source"
  echo '原账号和资源无法迁移。重新安装将移除残留容器，在上述独立实例目录创建新数据库和初始管理员。'
  # --yes approves normal updates only; recreating lost data always needs its own answer.
  while :; do
    printf '确认放弃旧数据并重新安装？Y=重新安装，N=取消 [Y/N，默认 N]：'
    if ! IFS= read -r answer; then answer=N; fi
    case "$answer" in
      Y|y) break ;;
      N|n|'') echo '已取消，未执行部署。'; exit 10 ;;
      *) echo '请输入 Y 或 N。' ;;
    esac
  done
fi
if [ "$mode" = migrate ]; then
  echo "现有数据：$source_data"
  echo '本次会先停服备份，再复制到独立实例目录；原数据目录保留。'
fi
if [ "$mode" != fresh ]; then
  echo '检测到已有部署。更新程序，保留账号、配置和资源。'
  if [ "$overwrite_confirmed" != true ]; then
    while :; do
      printf '是否覆盖更新？Y=覆盖，N=忽略 [Y/N，默认 N]：'
      if ! IFS= read -r answer; then answer=N; fi
      case "$answer" in
        Y|y) break ;;
        N|n|'') echo '已忽略，未执行部署。'; exit 10 ;;
        *) echo '请输入 Y 或 N。' ;;
      esac
    done
  fi
else
  echo '首次安装：将在上述实例目录创建数据库及初始管理员。'
fi
for file in settings.env.example docker-compose.yml tengine-server.tar image.sha256; do
  [ -f "$file" ] || fail "安装包缺少 $file。"
done
# The package Compose file is the single source for the versioned image tag.
image=$(sed -n 's/^    image: //p' docker-compose.yml)
case "$image" in tengine-server:*-amd64) ;; *) fail '安装包镜像名称无效。' ;; esac
command -v sha256sum >/dev/null 2>&1 || fail '缺少 sha256sum，不能校验镜像。'
sha256sum -c image.sha256
docker load -i tengine-server.tar
[ "$(docker image inspect --format '{{.Os}}/{{.Architecture}}' "$image")" = linux/amd64 ] || fail '镜像架构不匹配。'
compose() {
  if [ "$compose_kind" = plugin ]; then
    docker compose --env-file "$instance_dir/settings.env" -p tengine-http-multiuser -f "$instance_dir/docker-compose.yml" "$@"
  else
    "$compose_path" --env-file "$instance_dir/settings.env" -p tengine-http-multiuser -f "$instance_dir/docker-compose.yml" "$@"
  fi
}

stopped=false
completed=false
backup_dir=''
cleanup() {
  result=$?
  trap - EXIT
  if [ "$completed" != true ] && [ "$stopped" = true ] && [ "$was_running" = true ]; then
    # Before Compose replaces the container, a failed copy/backup can safely resume it.
    if docker inspect "$existing_container" >/dev/null 2>&1; then
      docker start "$existing_container" >/dev/null 2>&1 || true
    fi
  fi
  if [ "$result" -ne 0 ] && [ -n "$backup_dir" ]; then
    echo "本次备份保留在：$backup_dir" >&2
  fi
  exit "$result"
}
trap cleanup EXIT
trap 'exit 130' INT
trap 'exit 143' TERM
mkdir -p "$instance_dir"
if [ "$mode" != fresh ]; then
  if [ -n "$existing_container" ] && [ "$was_running" = true ]; then
    docker stop "$existing_container" >/dev/null
    stopped=true
  fi
  backup_dir="$instance_dir/backups/$(date +%Y%m%d-%H%M%S)-$$"
  mkdir -p "$backup_dir"
  echo "正在备份实例：$backup_dir"
  tar -cf "$backup_dir/data.tar" -C "$source_data" .
  cp -p "$source_settings" "$backup_dir/settings.env"
  if [ -f "$(dirname "$source_data")/docker-compose.yml" ]; then
    cp -p "$(dirname "$source_data")/docker-compose.yml" "$backup_dir/docker-compose.yml"
  fi
  if [ "$mode" = migrate ]; then
    mkdir -p "$data_dir"
    cp -a "$source_data/." "$data_dir/"
    # During migration the running instance's configuration is authoritative.
    if [ -f "$instance_dir/settings.env" ]; then
      cp -p "$instance_dir/settings.env" "$backup_dir/target-settings.env"
    fi
    cp -p "$source_settings" "$instance_dir/settings.env"
  fi
else
  cp settings.env.example "$instance_dir/settings.env"
  chmod 600 "$instance_dir/settings.env"
  if grep -Eq '^BOOTSTRAP_ADMIN_PASSWORD=[[:space:]]*$' "$instance_dir/settings.env"; then
    initial_password=$(od -An -N24 -tx1 /dev/urandom | tr -d ' \n')
    [ "${#initial_password}" -eq 48 ] || fail '生成初始密码失败。'
    sed "s/^BOOTSTRAP_ADMIN_PASSWORD=.*/BOOTSTRAP_ADMIN_PASSWORD=$initial_password/" "$instance_dir/settings.env" > "$instance_dir/settings.env.new"
    chmod 600 "$instance_dir/settings.env.new"
    mv "$instance_dir/settings.env.new" "$instance_dir/settings.env"
  fi
  mkdir -p "$data_dir"
fi
# Upgrade the old port key after backing up the instance. Never source the env file.
settings="$instance_dir/settings.env"
if grep -q '^HOST_PORT=' "$settings"; then
  if grep -q '^PORT=' "$settings"; then
    port_edit='/^HOST_PORT=/d'
  else
    port_edit='s/^HOST_PORT=/PORT=/'
  fi
  # Drop obsolete explanations shipped by the old template.
  (umask 077; sed -e "$port_edit" -e '/^# 【通常只需确认这一项】群晖监听端口/d' -e '/^# 容器内部端口固定为 /d' -e '/^# 不需要在此设置 DATA_DIR、PORT、TLS_CERT 或 TLS_KEY。$/d' "$settings" > "$settings.new")
  mv "$settings.new" "$settings"
  echo '端口配置已统一为 PORT，原配置已保存在更新备份中。'
fi
if [ "$mode" = fresh ]; then
  cp docker-compose.yml "$instance_dir/docker-compose.yml"
  compose config --quiet
  echo '正在首次初始化数据库…'
  compose run --rm -T --no-deps -e REQUIRE_EXISTING_DB=false tengine-server tengine-server --initialize
fi
[ -s "$data_dir/management.sqlite" ] || fail '数据库没有就绪，拒绝启动实例。'
cp docker-compose.yml "$instance_dir/docker-compose.yml"
compose config --quiet
if [ "$recreate_missing" = true ]; then
  # Initialization succeeded. Remove only the stale container, never directories or volumes.
  docker rm -f "$existing_container" >/dev/null
fi
compose up -d --force-recreate
attempt=0
until docker exec tengine-http-multiuser sh -c 'curl -fsS "http://127.0.0.1:${PORT:-8083}/api/health"' >/dev/null 2>&1; do
  attempt=$((attempt + 1))
  [ "$attempt" -lt 30 ] || { compose logs --tail 40; fail '实例未就绪，请检查以上日志及备份。'; }
  sleep 1
done
touch "$instance_dir/.tengine-instance"
completed=true
echo "部署成功。配置：$instance_dir/settings.env"
echo "数据库与资源：$data_dir"
service_port=$(docker exec tengine-http-multiuser printenv PORT)
compose port tengine-server "$service_port"
if [ -n "$backup_dir" ]; then echo "更新前备份：$backup_dir"; fi
echo '以后可以删除安装包目录；不要删除实例目录。更新不会重新创建管理员。'
