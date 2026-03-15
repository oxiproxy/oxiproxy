//! QUIC 协议隧道实现
//!
//! 此模块提供了基于 QUIC 协议的隧道实现，包括：
//! - `QuicSendStream` / `QuicRecvStream`: 流包装器
//! - `QuicConnection`: 连接包装器
//! - `QuicConnector`: 客户端连接器
//! - `QuicListener`: 服务端监听器

use anyhow::{anyhow, Result};
use async_trait::async_trait;
use quinn::{
    ClientConfig, Endpoint, ServerConfig, TransportConfig, VarInt,
    crypto::rustls::QuicClientConfig,
};
use rustls::pki_types::{CertificateDer, PrivateKeyDer, ServerName};
use std::collections::HashSet;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;
use tokio::io::AsyncWriteExt;
use sha2::{Sha256, Digest};

use super::traits::{TunnelConnection, TunnelConnector, TunnelListener, TunnelRecvStream, TunnelSendStream};

/// QUIC 发送流包装器
pub struct QuicSendStream {
    inner: quinn::SendStream,
}

impl QuicSendStream {
    /// 创建新的 QUIC 发送流包装器
    pub fn new(inner: quinn::SendStream) -> Self {
        Self { inner }
    }
}

#[async_trait]
impl TunnelSendStream for QuicSendStream {
    async fn write_all(&mut self, buf: &[u8]) -> Result<()> {
        self.inner.write_all(buf).await?;
        Ok(())
    }

    async fn flush(&mut self) -> Result<()> {
        self.inner.flush().await?;
        Ok(())
    }

    async fn finish(&mut self) -> Result<()> {
        self.inner.finish()?;
        Ok(())
    }
}

/// QUIC 接收流包装器
pub struct QuicRecvStream {
    inner: quinn::RecvStream,
}

impl QuicRecvStream {
    /// 创建新的 QUIC 接收流包装器
    pub fn new(inner: quinn::RecvStream) -> Self {
        Self { inner }
    }
}

#[async_trait]
impl TunnelRecvStream for QuicRecvStream {
    async fn read_exact(&mut self, buf: &mut [u8]) -> Result<()> {
        self.inner.read_exact(buf).await?;
        Ok(())
    }

    async fn read(&mut self, buf: &mut [u8]) -> Result<Option<usize>> {
        Ok(self.inner.read(buf).await?)
    }
}

/// QUIC 连接包装器
pub struct QuicConnection {
    inner: quinn::Connection,
}

impl QuicConnection {
    /// 创建新的 QUIC 连接包装器
    pub fn new(inner: quinn::Connection) -> Self {
        Self { inner }
    }

    /// 获取内部 quinn::Connection 引用
    pub fn inner(&self) -> &quinn::Connection {
        &self.inner
    }
}

#[async_trait]
impl TunnelConnection for QuicConnection {
    async fn open_bi(&self) -> Result<(Box<dyn TunnelSendStream>, Box<dyn TunnelRecvStream>)> {
        let (send, recv) = self.inner.open_bi().await?;
        Ok((
            Box::new(QuicSendStream::new(send)),
            Box::new(QuicRecvStream::new(recv)),
        ))
    }

    async fn accept_bi(&self) -> Result<(Box<dyn TunnelSendStream>, Box<dyn TunnelRecvStream>)> {
        let (send, recv) = self.inner.accept_bi().await?;
        Ok((
            Box::new(QuicSendStream::new(send)),
            Box::new(QuicRecvStream::new(recv)),
        ))
    }

    async fn open_uni(&self) -> Result<Box<dyn TunnelSendStream>> {
        let send = self.inner.open_uni().await?;
        Ok(Box::new(QuicSendStream::new(send)))
    }

    async fn accept_uni(&self) -> Result<Box<dyn TunnelRecvStream>> {
        let recv = self.inner.accept_uni().await?;
        Ok(Box::new(QuicRecvStream::new(recv)))
    }

    fn remote_address(&self) -> SocketAddr {
        self.inner.remote_address()
    }

    fn close_reason(&self) -> Option<String> {
        self.inner.close_reason().map(|r| r.to_string())
    }
}

/// 证书验证模式
#[derive(Debug, Clone)]
pub enum CertVerificationMode {
    /// 跳过验证（仅用于开发环境，不安全）
    SkipVerification,
    /// 证书指纹验证（推荐用于自签名证书）
    Fingerprint(HashSet<String>),
    /// 使用系统 CA 证书（用于公网 CA 签发的证书）
    SystemCA,
}

/// QUIC 客户端连接器
///
/// 用于客户端连接到 QUIC 服务器，支持多种证书验证模式。
pub struct QuicConnector {
    endpoint: Endpoint,
}

impl QuicConnector {
    /// 创建新的 QUIC 连接器（使用指定的证书验证模式）
    ///
    /// # Arguments
    /// * `verification_mode` - 证书验证模式
    ///
    /// # Examples
    /// ```
    /// // 使用证书指纹验证（推荐）
    /// let fingerprints = HashSet::from([
    ///     "sha256:1234567890abcdef...".to_string()
    /// ]);
    /// let connector = QuicConnector::new(CertVerificationMode::Fingerprint(fingerprints))?;
    ///
    /// // 使用系统 CA（用于公网证书）
    /// let connector = QuicConnector::new(CertVerificationMode::SystemCA)?;
    /// ```
    pub fn new(verification_mode: CertVerificationMode) -> Result<Self> {
        // 创建传输配置
        let mut transport_config = TransportConfig::default();
        transport_config.max_concurrent_uni_streams(0u32.into());
        transport_config.keep_alive_interval(Some(Duration::from_secs(5)));
        transport_config.max_idle_timeout(Some(Duration::from_secs(60).try_into()?));

        // 根据验证模式创建客户端配置
        let crypto = match verification_mode {
            CertVerificationMode::SkipVerification => {
                tracing::warn!("⚠️  QUIC 证书验证已禁用，仅用于开发环境！");
                rustls::ClientConfig::builder()
                    .dangerous()
                    .with_custom_certificate_verifier(Arc::new(SkipVerification))
                    .with_no_client_auth()
            }
            CertVerificationMode::Fingerprint(fingerprints) => {
                tracing::info!("🔒 QUIC 使用证书指纹验证（已配置 {} 个指纹）", fingerprints.len());
                rustls::ClientConfig::builder()
                    .dangerous()
                    .with_custom_certificate_verifier(Arc::new(FingerprintVerifier::new(fingerprints)))
                    .with_no_client_auth()
            }
            CertVerificationMode::SystemCA => {
                tracing::info!("🔒 QUIC 使用系统 CA 证书验证");
                rustls::ClientConfig::builder()
                    .with_root_certificates(rustls::RootCertStore::from_iter(
                        webpki_roots::TLS_SERVER_ROOTS.iter().cloned()
                    ))
                    .with_no_client_auth()
            }
        };

        let mut client_config = ClientConfig::new(Arc::new(QuicClientConfig::try_from(crypto)?));
        client_config.transport_config(Arc::new(transport_config));

        // 创建 QUIC 端点
        let mut endpoint = Endpoint::client("0.0.0.0:0".parse()?)?;
        endpoint.set_default_client_config(client_config);

        Ok(Self { endpoint })
    }

    /// 创建跳过验证的连接器（仅用于开发环境）
    #[deprecated(note = "不安全，仅用于开发环境。生产环境请使用 new() 并指定验证模式")]
    pub fn new_insecure() -> Result<Self> {
        Self::new(CertVerificationMode::SkipVerification)
    }
}

#[async_trait]
impl TunnelConnector for QuicConnector {
    async fn connect(&self, addr: SocketAddr) -> Result<Box<dyn TunnelConnection>> {
        let conn = self.endpoint.connect(addr, "oxiproxy")?.await?;
        Ok(Box::new(QuicConnection::new(conn)))
    }
}

/// QUIC 服务端监听器
///
/// 用于服务端接受 QUIC 客户端连接。
pub struct QuicListener {
    endpoint: Endpoint,
}

impl QuicListener {
    /// 创建新的 QUIC 监听器
    ///
    /// # Arguments
    /// * `bind_addr` - 绑定地址
    /// * `cert` - TLS 证书
    /// * `key` - TLS 私钥
    /// * `idle_timeout` - 空闲超时时间（秒）
    /// * `max_streams` - 最大并发流数
    /// * `keep_alive_interval` - 心跳间隔（秒）
    pub fn new(
        bind_addr: SocketAddr,
        cert: CertificateDer<'static>,
        key: PrivateKeyDer<'static>,
        idle_timeout: u64,
        max_streams: u32,
        keep_alive_interval: u64,
    ) -> Result<Self> {
        let mut transport_config = TransportConfig::default();
        transport_config.max_concurrent_uni_streams(VarInt::from_u32(max_streams));
        transport_config.keep_alive_interval(Some(Duration::from_secs(keep_alive_interval)));
        transport_config.max_idle_timeout(Some(Duration::from_secs(idle_timeout).try_into()?));

        let mut server_config = ServerConfig::with_single_cert(
            vec![cert],
            key,
        )?;
        server_config.transport_config(Arc::new(transport_config));

        let endpoint = Endpoint::server(server_config, bind_addr)?;

        Ok(Self { endpoint })
    }
}

#[async_trait]
impl TunnelListener for QuicListener {
    async fn accept(&self) -> Result<Box<dyn TunnelConnection>> {
        loop {
            if let Some(connecting) = self.endpoint.accept().await {
                match connecting.await {
                    Ok(conn) => {
                        return Ok(Box::new(QuicConnection::new(conn)));
                    }
                    Err(e) => {
                        tracing::error!("Connection accept failed: {}", e);
                        continue;
                    }
                }
            }
        }
    }
}

/// 证书指纹验证器
///
/// 验证服务器证书的 SHA-256 指纹是否在允许列表中。
/// 这种方式适合自签名证书，无需 CA 签发。
#[derive(Debug)]
struct FingerprintVerifier {
    allowed_fingerprints: HashSet<String>,
}

impl FingerprintVerifier {
    fn new(fingerprints: HashSet<String>) -> Self {
        Self {
            allowed_fingerprints: fingerprints,
        }
    }

    /// 计算证书的 SHA-256 指纹
    fn calculate_fingerprint(cert: &CertificateDer<'_>) -> String {
        let mut hasher = Sha256::new();
        hasher.update(cert.as_ref());
        let result = hasher.finalize();
        format!("sha256:{}", hex::encode(result))
    }
}

impl rustls::client::danger::ServerCertVerifier for FingerprintVerifier {
    fn verify_server_cert(
        &self,
        end_entity: &CertificateDer<'_>,
        _intermediates: &[CertificateDer<'_>],
        _server_name: &ServerName<'_>,
        _ocsp_response: &[u8],
        _now: rustls::pki_types::UnixTime,
    ) -> Result<rustls::client::danger::ServerCertVerified, rustls::Error> {
        let fingerprint = Self::calculate_fingerprint(end_entity);

        if self.allowed_fingerprints.contains(&fingerprint) {
            tracing::debug!("✅ 证书指纹验证通过: {}", fingerprint);
            Ok(rustls::client::danger::ServerCertVerified::assertion())
        } else {
            tracing::error!("❌ 证书指纹验证失败: {}", fingerprint);
            tracing::error!("允许的指纹: {:?}", self.allowed_fingerprints);
            Err(rustls::Error::InvalidCertificate(
                rustls::CertificateError::UnknownIssuer
            ))
        }
    }

    fn verify_tls12_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &rustls::DigitallySignedStruct,
    ) -> Result<rustls::client::danger::HandshakeSignatureValid, rustls::Error> {
        rustls::crypto::verify_tls12_signature(
            message,
            cert,
            dss,
            &rustls::crypto::ring::default_provider().signature_verification_algorithms,
        )
    }

    fn verify_tls13_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &rustls::DigitallySignedStruct,
    ) -> Result<rustls::client::danger::HandshakeSignatureValid, rustls::Error> {
        rustls::crypto::verify_tls13_signature(
            message,
            cert,
            dss,
            &rustls::crypto::ring::default_provider().signature_verification_algorithms,
        )
    }

    fn supported_verify_schemes(&self) -> Vec<rustls::SignatureScheme> {
        rustls::crypto::ring::default_provider()
            .signature_verification_algorithms
            .supported_schemes()
    }
}

/// 跳过证书验证器（不安全，仅用于开发环境）
///
/// ⚠️ 警告：此验证器会接受任何证书，包括伪造的证书。
/// 仅用于开发和测试环境，生产环境必须使用正确的证书验证。
#[derive(Debug)]
struct SkipVerification;

impl rustls::client::danger::ServerCertVerifier for SkipVerification {
    fn verify_server_cert(
        &self,
        _end_entity: &CertificateDer<'_>,
        _intermediates: &[CertificateDer<'_>],
        _server_name: &ServerName<'_>,
        _ocsp_response: &[u8],
        _now: rustls::pki_types::UnixTime,
    ) -> Result<rustls::client::danger::ServerCertVerified, rustls::Error> {
        Ok(rustls::client::danger::ServerCertVerified::assertion())
    }

    fn verify_tls12_signature(
        &self,
        _message: &[u8],
        _cert: &CertificateDer<'_>,
        _dss: &rustls::DigitallySignedStruct,
    ) -> Result<rustls::client::danger::HandshakeSignatureValid, rustls::Error> {
        Ok(rustls::client::danger::HandshakeSignatureValid::assertion())
    }

    fn verify_tls13_signature(
        &self,
        _message: &[u8],
        _cert: &CertificateDer<'_>,
        _dss: &rustls::DigitallySignedStruct,
    ) -> Result<rustls::client::danger::HandshakeSignatureValid, rustls::Error> {
        Ok(rustls::client::danger::HandshakeSignatureValid::assertion())
    }

    fn supported_verify_schemes(&self) -> Vec<rustls::SignatureScheme> {
        vec![
            rustls::SignatureScheme::RSA_PKCS1_SHA1,
            rustls::SignatureScheme::ECDSA_SHA1_Legacy,
            rustls::SignatureScheme::RSA_PKCS1_SHA256,
            rustls::SignatureScheme::ECDSA_NISTP256_SHA256,
            rustls::SignatureScheme::RSA_PKCS1_SHA384,
            rustls::SignatureScheme::ECDSA_NISTP384_SHA384,
            rustls::SignatureScheme::RSA_PKCS1_SHA512,
            rustls::SignatureScheme::ECDSA_NISTP521_SHA512,
            rustls::SignatureScheme::RSA_PSS_SHA256,
            rustls::SignatureScheme::RSA_PSS_SHA384,
            rustls::SignatureScheme::RSA_PSS_SHA512,
            rustls::SignatureScheme::ED25519,
            rustls::SignatureScheme::ED448,
        ]
    }
}

/// 辅助函数：从证书文件计算指纹
///
/// # Examples
/// ```
/// let fingerprint = calculate_cert_fingerprint_from_file("cert.pem")?;
/// println!("证书指纹: {}", fingerprint);
/// ```
pub fn calculate_cert_fingerprint_from_file(cert_path: &str) -> Result<String> {
    let cert_pem = std::fs::read(cert_path)?;
    let cert = rustls_pemfile::certs(&mut cert_pem.as_slice())
        .next()
        .ok_or_else(|| anyhow!("证书文件为空"))??;

    let mut hasher = Sha256::new();
    hasher.update(cert.as_ref());
    let result = hasher.finalize();
    Ok(format!("sha256:{}", hex::encode(result)))
}

/// 辅助函数：从 PEM 内容计算指纹
pub fn calculate_cert_fingerprint_from_pem(cert_pem: &[u8]) -> Result<String> {
    let mut pem_reader = cert_pem;
    let cert = rustls_pemfile::certs(&mut pem_reader)
        .next()
        .ok_or_else(|| anyhow!("证书内容为空"))??;

    let mut hasher = Sha256::new();
    hasher.update(cert.as_ref());
    let result = hasher.finalize();
    Ok(format!("sha256:{}", hex::encode(result)))
}
