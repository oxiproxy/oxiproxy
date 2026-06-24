#!/usr/bin/env bash
#
# OxiProxy dated 日志归档脚本
#
# 处理 tracing-appender (rolling::daily) 产生的按天日志：
#   node.log.YYYY-MM-DD / client.log.YYYY-MM-DD / controller.log.YYYY-MM-DD
# 以及 daemon 模式的 daemon.err（普通 logrotate 的 *.log glob 匹配不到这些）。
#
# 为什么不直接用 logrotate：
#   这些文件已经被应用按天轮转了。"今天"的文件应用还持有 fd 在写，logrotate 的
#   rename/copytruncate 会破坏正在写入的文件；而"昨天及更早"的文件应用已不再写、
#   可安全压缩删除。本脚本用 `find -daystart -mtime` 精确地只动"非今天"的文件。
#
# 行为：
#   1) 把"非今天"的、未压缩的 dated 文件 gzip 压缩；
#   2) 删除超过 KEEP_DAYS 天的 .gz 归档。
#
# 安装为每日定时（二选一）：
#   # cron.d:
#   sudo cp deploy/scripts/oxiproxy-log-archive.sh /usr/local/bin/oxiproxy-log-archive
#   sudo chmod 755 /usr/local/bin/oxiproxy-log-archive
#   sudo cp deploy/cron.d/oxiproxy-log-archive /etc/cron.d/oxiproxy-log-archive
#
# 手动验证（不删不压，只打印将处理的文件）：
#   DRY_RUN=1 deploy/scripts/oxiproxy-log-archive.sh

set -euo pipefail

LOG_DIR="${OXIPROXY_LOG_DIR:-/opt/oxiproxy/log}"
KEEP_DAYS="${OXIPROXY_LOG_KEEP_DAYS:-14}"
DRY_RUN="${DRY_RUN:-0}"

if [[ ! -d "$LOG_DIR" ]]; then
    echo "日志目录不存在，跳过: $LOG_DIR" >&2
    exit 0
fi

log() { echo "[oxiproxy-log-archive] $*"; }

# 1) 压缩"非今天"的 dated 日志
#    -daystart + -mtime +0 = 最后修改时间早于今天 00:00（即排除今天仍在写的文件）
#    匹配形如 *.log.2026-06-24，且尚未压缩（排除 *.gz）
while IFS= read -r -d '' f; do
    if [[ "$DRY_RUN" == "1" ]]; then
        log "将压缩: $f"
    else
        gzip -- "$f" && log "已压缩: ${f}.gz"
    fi
done < <(find "$LOG_DIR" -maxdepth 1 -type f \
            \( -name '*.log.[0-9]*' -o -name 'daemon.err' \) \
            ! -name '*.gz' -daystart -mtime +0 -print0)

# 2) 删除超过 KEEP_DAYS 天的归档
while IFS= read -r -d '' f; do
    if [[ "$DRY_RUN" == "1" ]]; then
        log "将删除(超过 ${KEEP_DAYS} 天): $f"
    else
        rm -f -- "$f" && log "已删除: $f"
    fi
done < <(find "$LOG_DIR" -maxdepth 1 -type f -name '*.gz' \
            -daystart -mtime +"$KEEP_DAYS" -print0)

log "完成（目录=$LOG_DIR, 保留=${KEEP_DAYS}天, dry_run=$DRY_RUN）"
