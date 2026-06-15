use anyhow::Result;
use rcgen::{CertificateParams, DistinguishedName, DnType, KeyPair};
use time::OffsetDateTime;

/// 生成自签名证书
///
/// # Arguments
/// * `common_name` - 证书的 CN（Common Name）
/// * `validity_days` - 有效期（天数）
///
/// # Returns
/// * `(cert_pem, key_pem, fingerprint)` - 证书 PEM、私钥 PEM、SHA-256 指纹
pub fn generate_self_signed_certificate(
    common_name: &str,
    validity_days: u32,
) -> Result<(String, String, String)> {
    // 创建证书参数
    // SAN（dNSName）必须是 IA5String（仅 ASCII），节点名可能含中文等非 ASCII 字符，
    // 这里清洗为 ASCII 安全的 DNS 名，避免证书生成失败。
    // 注意：Client 通过 SHA-256 指纹验证证书（FingerprintVerifier），不校验 hostname，
    // 因此 SAN 的具体取值不影响安全性。
    let san = sanitize_dns_san(common_name);
    let mut params = CertificateParams::new(vec![san])?;

    // 设置 Distinguished Name（CN 为 UTF8String，可保留完整名称含中文）
    let mut dn = DistinguishedName::new();
    dn.push(DnType::CommonName, common_name);
    params.distinguished_name = dn;

    // 设置有效期（使用 time crate 的 OffsetDateTime）
    let not_before = OffsetDateTime::now_utc();
    let not_after = not_before + time::Duration::days(validity_days as i64);
    params.not_before = not_before;
    params.not_after = not_after;

    // 生成密钥对和自签名证书
    let key_pair = KeyPair::generate()?;
    let cert = params.self_signed(&key_pair)?;
    let cert_pem = cert.pem();
    let key_pem = key_pair.serialize_pem();

    // 计算指纹
    let fingerprint = common::tunnel::calculate_cert_fingerprint_from_pem(
        cert_pem.as_bytes()
    )?;

    tracing::info!("✅ 生成自签名证书: CN={}, 有效期={}天", common_name, validity_days);
    tracing::debug!("   指纹: {}", fingerprint);

    Ok((cert_pem, key_pem, fingerprint))
}

/// 为节点生成证书
pub fn generate_node_certificate(node_id: i64, node_name: &str) -> Result<(String, String, String)> {
    let cn = format!("oxiproxy-node-{}-{}", node_id, node_name);
    generate_self_signed_certificate(&cn, 3650) // 10 年有效期
}

/// 将任意字符串清洗为 ASCII 安全的 DNS SAN（dNSName 要求 IA5String）。
/// 非 ASCII 字母数字、'-'、'.' 以外的字符统一替换为 '-'；结果为空时回退到固定值。
fn sanitize_dns_san(input: &str) -> String {
    let s: String = input
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '.' {
                c
            } else {
                '-'
            }
        })
        .collect();
    if s.is_empty() {
        "oxiproxy-node".to_string()
    } else {
        s
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_generate_certificate() {
        let (cert_pem, key_pem, fingerprint) =
            generate_self_signed_certificate("test-node", 365).unwrap();

        assert!(cert_pem.contains("BEGIN CERTIFICATE"));
        assert!(key_pem.contains("BEGIN PRIVATE KEY"));
        assert!(fingerprint.starts_with("sha256:"));
    }

    #[test]
    fn test_generate_certificate_with_non_ascii_name() {
        // 含中文的节点名不应导致证书生成失败（SAN 会被清洗为 ASCII）
        let (cert_pem, _key_pem, fingerprint) =
            generate_node_certificate(4, "雨云-200M-188").unwrap();
        assert!(cert_pem.contains("BEGIN CERTIFICATE"));
        assert!(fingerprint.starts_with("sha256:"));
    }

    #[test]
    fn test_sanitize_dns_san() {
        assert_eq!(sanitize_dns_san("oxiproxy-node-4-雨云-200M-188"), "oxiproxy-node-4----200M-188");
        assert_eq!(sanitize_dns_san("plain.example-1"), "plain.example-1");
        assert_eq!(sanitize_dns_san("全中文"), "---");
    }
}
