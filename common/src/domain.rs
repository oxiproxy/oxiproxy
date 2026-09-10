//! Exact DNS names used by shared HTTP and TLS listeners.
pub fn normalize_domain(value: &str) -> Result<String, &'static str> {
    let value = value
        .trim()
        .strip_suffix('.')
        .unwrap_or(value.trim())
        .to_ascii_lowercase();
    if value.is_empty()
        || value.len() > 253
        || value.parse::<std::net::IpAddr>().is_ok()
        || value.split('.').any(|label| {
            label.is_empty()
                || label.len() > 63
                || label.starts_with('-')
                || label.ends_with('-')
                || !label
                    .bytes()
                    .all(|c| c.is_ascii_alphanumeric() || c == b'-')
        })
    {
        return Err("请输入有效域名（不含协议、路径、端口或通配符；国际化域名请使用 Punycode）");
    }
    Ok(value)
}

pub fn validate_route(kind: &str, domain: &str) -> Result<String, &'static str> {
    match kind {
        "http" | "https" => normalize_domain(domain),
        "tcp" | "udp" if domain.is_empty() => Ok(String::new()),
        "tcp" | "udp" => Err("TCP/UDP 代理不能配置域名"),
        _ => Err("代理类型必须为 tcp、udp、http 或 https"),
    }
}

pub fn routes_conflict(kind: &str, domain: &str, other_kind: &str, other_domain: &str) -> bool {
    !matches!(kind, "http" | "https") || kind != other_kind || domain == other_domain
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn domain_validation() {
        assert_eq!(normalize_domain(" Example.COM. "), Ok("example.com".into()));
        for invalid in [
            "",
            "*.example.com",
            "https://example.com",
            "a.com:80",
            "a..com",
            "a.com..",
            "-a.com",
            "a_.com",
            "127.0.0.1",
            "例子.com",
        ] {
            assert!(normalize_domain(invalid).is_err(), "{invalid}");
        }
        assert!(validate_route("tcp", "a.com").is_err());
        assert!(validate_route("https", "").is_err());
        assert!(validate_route("other", "").is_err());
    }
    #[test]
    fn only_distinct_domains_of_the_same_protocol_share_a_port() {
        assert!(!routes_conflict("http", "a.com", "http", "b.com"));
        assert!(routes_conflict("https", "a.com", "https", "a.com"));
        assert!(routes_conflict("http", "a.com", "https", "b.com"));
        assert!(routes_conflict("http", "a.com", "tcp", ""));
        assert!(routes_conflict("tcp", "", "http", "a.com"));
    }
}
