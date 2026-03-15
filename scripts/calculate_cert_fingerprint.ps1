# 证书指纹计算工具（Windows PowerShell）
# 用于计算 QUIC 证书的 SHA-256 指纹

param(
    [Parameter(Mandatory=$true)]
    [string]$CertPath
)

if (-not (Test-Path $CertPath)) {
    Write-Error "证书文件不存在: $CertPath"
    exit 1
}

# 读取证书
$cert = New-Object System.Security.Cryptography.X509Certificates.X509Certificate2($CertPath)

# 计算 SHA-256 指纹
$fingerprint = $cert.GetCertHashString([System.Security.Cryptography.HashAlgorithmName]::SHA256).ToLower()

Write-Host "sha256:$fingerprint"
Write-Host ""
Write-Host "✅ 请将此指纹添加到 Client 配置中" -ForegroundColor Green
Write-Host "   例如: --cert-fingerprint sha256:$fingerprint"
