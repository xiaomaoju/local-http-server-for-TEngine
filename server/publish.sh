#!/bin/bash
set -e

# ============================================================
#  TEngine 本地打包 + 上传服务器脚本
#
#  在你的 Mac 上运行，不需要 git push
#  用法:
#    bash publish.sh                     # 使用默认配置
#    bash publish.sh user@1.2.3.4        # 指定服务器地址
#    bash publish.sh user@1.2.3.4 /opt/tengine  # 指定服务器地址和目录
# ============================================================

GREEN='\033[0;32m'
YELLOW='\033[1;33m'
RED='\033[0;31m'
NC='\033[0m'

info()  { echo -e "${GREEN}[INFO]${NC} $1"; }
warn()  { echo -e "${YELLOW}[WARN]${NC} $1"; }
error() { echo -e "${RED}[ERROR]${NC} $1"; exit 1; }

# ---------- 配置（可以直接改这里，就不用每次输参数）----------
DEFAULT_SERVER="root@43.173.103.249"
DEFAULT_REMOTE_DIR="/opt/tengine"
IMAGE_NAME="tengine-server"
# ----------------------------------------------------------

SERVER="${1:-$DEFAULT_SERVER}"
REMOTE_DIR="${2:-$DEFAULT_REMOTE_DIR}"

if [[ "$SERVER" == *"你的服务器"* ]]; then
    echo ""
    echo "请先配置服务器地址，有两种方式："
    echo ""
    echo "  方式一：直接传参"
    echo "    bash publish.sh root@1.2.3.4"
    echo ""
    echo "  方式二：编辑 publish.sh 开头的 DEFAULT_SERVER"
    echo "    DEFAULT_SERVER=\"root@1.2.3.4\""
    echo ""
    exit 1
fi

# ---------- 0. 检查本地 Docker ----------
if ! command -v docker &> /dev/null; then
    error "本地未安装 Docker Desktop，请先安装: https://www.docker.com/products/docker-desktop/"
fi

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
PROJECT_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"

# ---------- 1. 本地构建镜像 ----------
info "第 1 步：在本地构建 Docker 镜像..."
info "（首次构建大约 5-10 分钟，之后有缓存会快很多）"
echo ""

docker build --platform linux/amd64 -t "${IMAGE_NAME}:latest" -f "$SCRIPT_DIR/Dockerfile" "$PROJECT_ROOT"

info "镜像构建完成！"

# ---------- 2. 导出为文件 ----------
TARFILE="${SCRIPT_DIR}/${IMAGE_NAME}.tar.gz"

info "第 2 步：打包镜像为文件..."
docker save "${IMAGE_NAME}:latest" | gzip > "$TARFILE"
FILESIZE=$(du -h "$TARFILE" | cut -f1)
info "打包完成: ${FILESIZE}"

# ---------- 3. 上传到服务器 ----------
info "第 3 步：上传到服务器 ${SERVER}..."
info "（取决于你的网速，可能需要几分钟）"
echo ""

# 确保服务器上目录存在
ssh "$SERVER" "mkdir -p ${REMOTE_DIR}/server"

# 上传镜像文件
scp "$TARFILE" "${SERVER}:${REMOTE_DIR}/server/"

# 上传配置文件（不覆盖已有的 .env）
scp "$SCRIPT_DIR/docker-compose.prod.yml" "${SERVER}:${REMOTE_DIR}/server/"
scp "$SCRIPT_DIR/deploy.sh" "${SERVER}:${REMOTE_DIR}/server/"
scp "$SCRIPT_DIR/backup.sh" "${SERVER}:${REMOTE_DIR}/server/"

info "上传完成！"

# ---------- 4. 在服务器上加载并重启 ----------
info "第 4 步：在服务器上加载镜像并重启服务..."

ssh "$SERVER" bash << REMOTE_SCRIPT
set -e
cd ${REMOTE_DIR}/server

echo "[服务器] 加载镜像..."
docker load < ${IMAGE_NAME}.tar.gz

echo "[服务器] 清理镜像文件..."
rm -f ${IMAGE_NAME}.tar.gz

# 如果 .env 不存在，从 deploy.sh 走首次配置流程
if [ ! -f .env ]; then
    echo ""
    echo "=========================================="
    echo "  首次部署，请在服务器上运行:"
    echo "  cd ${REMOTE_DIR}/server && bash deploy.sh"
    echo "=========================================="
    exit 0
fi

# 更新 docker-compose.prod.yml 让它用本地镜像而不是构建
echo "[服务器] 重启服务..."
docker compose -f docker-compose.prod.yml up -d
echo "[服务器] 完成！"
docker compose -f docker-compose.prod.yml ps
REMOTE_SCRIPT

# ---------- 5. 清理本地文件 ----------
rm -f "$TARFILE"

echo ""
info "全部完成！"
echo ""
echo "  你的服务已经更新到最新代码了。"
echo "  访问: http://${SERVER#*@}"
echo ""
