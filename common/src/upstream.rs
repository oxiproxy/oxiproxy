//! Validated origins for Node-owned reverse proxies.
use anyhow::{ensure, Result};
use reqwest::Url;

pub fn parse_upstream(kind: &str, value: &str) -> Result<Url> {
    ensure!(
        matches!(kind, "http" | "https"),
        "公网直连仅支持 HTTP/HTTPS 域名代理"
    );
    let url = Url::parse(value.trim())?;
    ensure!(
        matches!(url.scheme(), "http" | "https"),
        "目标地址必须以 http:// 或 https:// 开头"
    );
    ensure!(
        kind != "https" || url.scheme() == "https",
        "HTTPS 透传目标必须使用 https://"
    );
    ensure!(
        url.host_str().is_some() && url.port_or_known_default().is_some_and(|port| port > 0),
        "目标地址缺少有效主机或端口"
    );
    ensure!(
        url.username().is_empty() && url.password().is_none(),
        "目标地址不能包含登录凭据"
    );
    ensure!(
        url.path() == "/" && url.query().is_none() && url.fragment().is_none(),
        "目标地址仅填写协议、主机和端口，不包含路径、查询参数或片段"
    );
    Ok(url)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validates_origins_and_tls_passthrough() {
        for origin in [
            "http://example.com:8080",
            "https://example.com",
            "http://[::1]:8080",
        ] {
            assert!(parse_upstream("http", origin).is_ok(), "{origin}");
        }
        assert_eq!(
            parse_upstream("https", " HTTPS://Example.COM:8443/ ")
                .unwrap()
                .as_str(),
            "https://example.com:8443/"
        );
        for origin in [
            "example.com",
            "ftp://example.com",
            "http://example.com:0",
            "https://user:pass@example.com",
            "http://example.com/api",
            "http://example.com/?a=1",
            "http://example.com/#x",
        ] {
            assert!(parse_upstream("http", origin).is_err(), "{origin}");
        }
        assert!(parse_upstream("https", "http://example.com").is_err());
        assert!(parse_upstream("tcp", "http://example.com").is_err());
    }
}
