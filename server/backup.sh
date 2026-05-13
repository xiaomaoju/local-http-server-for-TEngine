#!/bin/bash
# ============================================================
#  TEngine 数据备份脚本
#  用法: bash backup.sh
#  建议加入 crontab 定时执行: 0 3 * * * cd /opt/tengine/server && bash backup.sh
# ============================================================

BACKUP_DIR="/opt/backups/tengine"
KEEP_DAYS=7

mkdir -p "$BACKUP_DIR"

TIMESTAMP=$(date +%Y%m%d_%H%M%S)
BACKUP_FILE="${BACKUP_DIR}/tengine-backup-${TIMESTAMP}.tar.gz"

echo "[$(date)] 开始备份..."
docker run --rm \
    -v tengine-data:/data:ro \
    -v "${BACKUP_DIR}":/backup \
    alpine tar czf "/backup/tengine-backup-${TIMESTAMP}.tar.gz" -C /data .

FILESIZE=$(du -h "$BACKUP_FILE" | cut -f1)
echo "[$(date)] 备份完成: ${BACKUP_FILE} (${FILESIZE})"

# 清理旧备份
DELETED=$(find "$BACKUP_DIR" -name "tengine-backup-*.tar.gz" -mtime +${KEEP_DAYS} -delete -print | wc -l)
if [ "$DELETED" -gt 0 ]; then
    echo "[$(date)] 已清理 ${DELETED} 个超过 ${KEEP_DAYS} 天的旧备份"
fi
