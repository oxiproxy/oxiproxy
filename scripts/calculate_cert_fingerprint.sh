#!/usr/bin/env bash
# 证书指纹计算工具
# 用于计算 QUIC 证书的 SHA-256 指纹

set -e

if [ $# -lt 1 ]; then
    echo "用法: $0 <证书文件路径>"
    echo ""
    echo "示例:"
    echo "  $0 /path/to/cert.pem"
    echo ""
    echo "输出格式: sha256:abcdef1234567890..."
    exit 1
fi

CERT_FILE="$1"

if [ ! -f "$CERT_FILE" ]; then
    echo "错误: 证书文件不存在: $CERT_FILE"
    exit 1
fi

# 计算证书指纹
FINGERPRINT=$(openssl x509 -in "$CERT_FILE" -noout -fingerprint -sha256 | \
    sed 's/SHA256 Fingerprint=//' | \
    tr -d ':' | \
    tr '[:upper:]' '[:lower:]')

echo "sha256:$FINGERPRINT"
echo ""
echo "✅ 请将此指纹添加到 Client 配置中"
echo "   例如: --cert-fingerprint sha256:$FINGERPRINT"
