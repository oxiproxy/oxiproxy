use anyhow::Result;
use rcgen::{Certificate, CertificateParams, DistinguishedName, DnType};
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
    let mut params = CertificateParams::new(vec![common_name.to_string()]);

    // 设置 Distinguished Name
    let mut dn = DistinguishedName::new();
    dn.push(DnType::CommonName, common_name);
    params.distinguished_name = dn;

    // 设置有效期（使用 time crate 的 OffsetDateTime）
    let not_before = OffsetDateTime::now_utc();
    let not_after = not_before + time::Duration::days(validity_days as i64);
    params.not_before = not_before;
    params.not_after = not_after;

    // 生成证书
    let cert = Certificate::from_params(params)?;
    let cert_pem = cert.serialize_pem()?;
    let key_pem = cert.serialize_private_key_pem();

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
}
