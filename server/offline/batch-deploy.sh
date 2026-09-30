#!/usr/bin/env bash
# Run on the publishing computer (macOS / Linux): bash batch-deploy.sh
set -u -o pipefail
package_dir=$(CDPATH= cd "$(dirname "$0")" && pwd)
cd "$package_dir" || exit 1
if ! command -v ssh >/dev/null || ! command -v tar >/dev/null; then
  echo "需要 SSH 和 tar；请在 macOS / Linux 终端执行。"
  exit 1
fi
for file in servers.txt install.env tengine-server.tar image.sha256 docker-compose.yml deploy.sh BUILD-INFO.json README-部署说明.md 发布指南.md nginx.conf.example settings.env.example; do
  if [ ! -f "$file" ]; then echo "缺少发布文件：$file"; exit 1; fi
done

publish_server() {
  local login="$1" port="$2" remote_dir="$3"
  local deploy_option="" answer probe_status
  if ssh -n -p "$port" "$login" "if [ -e '$remote_dir/deploy.sh' ] || [ -e '$remote_dir/tengine-server.tar' ] || [ -e '$remote_dir/settings.env' ] || [ -e '$remote_dir/data/management.sqlite' ]; then exit 0; else exit 3; fi"; then
    echo "$login 的目标目录已有部署。覆盖只更新程序文件，保留独立实例目录内的 settings.env 和 data。"
    while :; do
      printf '是否覆盖更新？Y=覆盖，N=忽略 [Y/N，默认 N]：'
      if ! IFS= read -r answer; then answer=N; fi
      case "$answer" in
        Y|y) deploy_option="--yes"; break ;;
        N|n|'') return 10 ;;
        *) echo "请输入 Y 或 N。" ;;
      esac
    done
  else
    probe_status=$?
    if [ "$probe_status" -ne 3 ]; then
      echo "无法检查 $login 的部署状态，未上传或覆盖文件。"
      return 1
    fi
  fi
  ssh -n -p "$port" "$login" "mkdir -p '$remote_dir'" || return 1
  tar -cf - tengine-server.tar image.sha256 docker-compose.yml deploy.sh BUILD-INFO.json README-部署说明.md 发布指南.md nginx.conf.example settings.env.example |
    ssh -p "$port" "$login" "tar -xf - -C '$remote_dir'" || return 1
  # Keep each host's instance directory choice. Never upload runtime settings or data.
  ssh -p "$port" "$login" "if [ -f '$remote_dir/install.env' ]; then cat >/dev/null; else umask 077; cat > '$remote_dir/install.env'; fi" < install.env || return 1
  local deploy_command="cd '$remote_dir' && if [ \"\$(id -u)\" -eq 0 ]; then sh deploy.sh $deploy_option; else sudo sh deploy.sh $deploy_option; fi"
  if [ -t 0 ]; then
    ssh -t -p "$port" "$login" "$deploy_command" || return $?
  else
    ssh -n -p "$port" "$login" "$deploy_command" || return $?
  fi
}

success=0
failed=0
skipped=0
# Keep terminal stdin available for Y/N; never read answers from servers.txt.
while read -r login port remote_dir extra <&3 || [ -n "${login:-}" ]; do
  case "${login:-}" in ''|\#*) continue ;; esac
  port=${port:-22}
  remote_dir=${remote_dir:-/volume1/docker/TEngineHttpV2}
  remote_dir=${remote_dir%$'\r'}
  if [ -n "${extra:-}" ] || ! printf '%s' "$login" | grep -Eq '^[A-Za-z0-9_][A-Za-z0-9_.@:-]*$' || ! printf '%s' "$port" | grep -Eq '^[0-9]{1,5}$' || ! [[ "$remote_dir" == /*/TEngineHttpV2 && "$remote_dir" != *"'"* ]]; then
    echo "服务器清单格式错误：${login}，请按 servers.txt 注释填写。"
    failed=$((failed + 1))
    continue
  fi
  case "$remote_dir" in */../*|*/./*) echo "拒绝包含相对跳转的路径：$remote_dir"; failed=$((failed + 1)); continue ;; esac
  echo "===== 正在部署 $login:$port ====="
  if publish_server "$login" "$port" "$remote_dir"; then
    echo "成功：$login"
    success=$((success + 1))
  else
    deploy_status=$?
    if [ "$deploy_status" -eq 10 ]; then
      echo "已忽略：$login"
      skipped=$((skipped + 1))
    else
      echo "失败：${login}；请检查 SSH、目录权限或上方日志。继续下一台。"
      failed=$((failed + 1))
    fi
  fi
done 3< servers.txt
echo "发布完成：成功 $success 台，忽略 $skipped 台，失败 $failed 台。"
if [ "$success" -eq 0 ] && [ "$failed" -eq 0 ] && [ "$skipped" -eq 0 ]; then
  echo "尚未填写服务器。编辑 servers.txt 后重新执行。"
  exit 1
fi
[ "$failed" -eq 0 ]
