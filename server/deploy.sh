#!/bin/bash
set -e

# ============================================================
#  TEngine HTTP 一键部署脚本（适配宝塔面板）
#  用法: bash deploy.sh
#  前提: 服务器已安装 Docker（宝塔镜像自带）
# ============================================================

GREEN='\033[0;32m'
YELLOW='\033[1;33m'
RED='\033[0;31m'
NC='\033[0m'

info()  { echo -e "${GREEN}[INFO]${NC} $1"; }
warn()  { echo -e "${YELLOW}[WARN]${NC} $1"; }
error() { echo -e "${RED}[ERROR]${NC} $1"; exit 1; }

# ---------- 1. 检查 Docker 是否已安装 ----------
info "检查 Docker 环境..."
if ! command -v docker &> /dev/null; then
    warn "Docker 未安装，正在自动安装..."
    curl -fsSL https://get.docker.com | sh
    systemctl enable docker
    systemctl start docker
    info "Docker 安装完成"
fi

if ! docker compose version &> /dev/null; then
    error "docker compose 不可用。请升级 Docker 到最新版本。"
fi

info "Docker 版本: $(docker --version)"
info "Docker Compose 版本: $(docker compose version)"

# ---------- 2. 检查是否在正确目录 ----------
if [ ! -f "docker-compose.prod.yml" ]; then
    error "请在 server/ 目录下运行此脚本！\n  cd /opt/tengine/server && bash deploy.sh"
fi

# ---------- 3. 创建 .env 文件（如果不存在）----------
if [ ! -f ".env" ]; then
    info "首次部署，正在创建 .env 配置文件..."

    JWT_SECRET=$(openssl rand -hex 32 2>/dev/null || head -c 64 /dev/urandom | od -An -tx1 | tr -d ' \n')

    echo ""
    echo "=========================================="
    echo "  请设置管理员密码"
    echo "  （这是你登录管理后台的密码）"
    echo "=========================================="
    read -sp "请输入管理员密码: " ADMIN_PASSWORD
    echo ""

    if [ -z "$ADMIN_PASSWORD" ]; then
        error "密码不能为空！"
    fi

    cat > .env << EOF
ADMIN_PASSWORD=${ADMIN_PASSWORD}
JWT_SECRET=${JWT_SECRET}
PORT=8082
DATA_DIR=/data
TOKEN_EXPIRE_HOURS=24
EOF

    chmod 600 .env
    info ".env 文件已创建（权限已设置为仅 root 可读）"
else
    info ".env 文件已存在，跳过创建"
fi

# ---------- 4. 启动服务 ----------
info "启动服务..."
docker compose -f docker-compose.prod.yml up -d

# ---------- 5. 等待健康检查通过 ----------
info "等待服务启动..."
for i in $(seq 1 30); do
    if docker compose -f docker-compose.prod.yml ps | grep -q "healthy"; then
        break
    fi
    sleep 2
done

# ---------- 6. 检查状态 ----------
echo ""
echo "=========================================="
docker compose -f docker-compose.prod.yml ps
echo "=========================================="

SERVER_IP=$(curl -s --connect-timeout 3 ifconfig.me 2>/dev/null || echo "你的服务器IP")

echo ""
info "Docker 服务部署完成！"
info "服务运行在 127.0.0.1:8082（仅本机可访问）"
echo ""
echo "  =================================================="
echo "  接下来需要在宝塔面板中配置反向代理："
echo ""
echo "  1. 打开宝塔面板: http://${SERVER_IP}:8888"
echo "  2. 左侧菜单 → 网站 → 添加站点"
echo "     - 域名填: ${SERVER_IP} (或你的域名)"
echo "     - 不需要选数据库和 PHP"
echo "  3. 点击刚创建的站点 → 反向代理 → 添加反向代理"
echo "     - 代理名称: tengine"
echo "     - 目标URL: http://127.0.0.1:8082"
echo "     - 勾选「启用反向代理」"
echo "  4. 保存，然后浏览器访问 http://${SERVER_IP}"
echo "  =================================================="
echo ""
echo "  常用命令:"
echo "    查看日志:     docker compose -f docker-compose.prod.yml logs -f"
echo "    重启服务:     docker compose -f docker-compose.prod.yml restart"
echo "    停止服务:     docker compose -f docker-compose.prod.yml down"
echo ""
