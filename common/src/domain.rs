//! DNS names and wildcard routes used by shared HTTP and TLS listeners.
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

/// Route patterns permit a single leading `*.`; wire Host/SNI names remain exact.
pub fn normalize_pattern(value: &str) -> Result<String, &'static str> {
    if let Some(suffix) = value.trim().strip_prefix("*.") {
        if suffix.starts_with(char::is_whitespace) {
            return Err("通配符与域名之间不能包含空格");
        }
        let suffix = normalize_domain(suffix)?;
        if suffix.len() + 2 > 253 {
            return Err("通配符域名过长");
        }
        Ok(format!("*.{suffix}"))
    } else {
        normalize_domain(value)
    }
}

/// Exact names win, then the longest wildcard suffix. A wildcard matches one or
/// more labels, never its apex. Lookup order is independent of insertion order.
pub fn find_route<'a, T>(
    routes: &'a std::collections::HashMap<String, T>,
    domain: &str,
) -> Option<&'a T> {
    if let Some(route) = routes.get(domain) {
        return Some(route);
    }
    for (index, _) in domain.match_indices('.') {
        if let Some(route) = routes.get(&format!("*{}", &domain[index..])) {
            return Some(route);
        }
    }
    None
}

pub fn validate_route(kind: &str, domain: &str) -> Result<String, &'static str> {
    match kind {
        "http" | "https" => normalize_pattern(domain),
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
    fn wildcard_validation_and_conflicts() {
        assert_eq!(
            validate_route("https", " *.Yunnet.TOP. "),
            Ok("*.yunnet.top".into())
        );
        for invalid in [
            "*",
            "*.*.top",
            "a*.yunnet.top",
            "a.*.top",
            "*.127.0.0.1",
            "*. example.com",
            "*.a..top",
        ] {
            assert!(validate_route("http", invalid).is_err(), "{invalid}");
        }
        assert!(routes_conflict(
            "http",
            "*.yunnet.top",
            "http",
            "*.yunnet.top"
        ));
        assert!(!routes_conflict(
            "http",
            "*.yunnet.top",
            "http",
            "nas.yunnet.top"
        ));
        assert!(!routes_conflict(
            "http",
            "*.yunnet.top",
            "http",
            "*.home.yunnet.top"
        ));
        assert!(routes_conflict(
            "http",
            "*.yunnet.top",
            "https",
            "nas.yunnet.top"
        ));
    }

    #[test]
    fn exact_then_longest_suffix_without_apex_or_boundary_leaks() {
        let routes = std::collections::HashMap::from([
            ("*.yunnet.top".into(), 1),
            ("nas.yunnet.top".into(), 2),
            ("*.home.yunnet.top".into(), 3),
        ]);
        for (host, expected) in [
            ("xxx.yunnet.top", Some(&1)),
            ("nas.yunnet.top", Some(&2)),
            ("a.home.yunnet.top", Some(&3)),
            ("a.b.home.yunnet.top", Some(&3)),
            ("home.yunnet.top", Some(&1)),
            ("yunnet.top", None),
            ("notyunnet.top", None),
            ("yunnet.top.evil.test", None),
        ] {
            assert_eq!(find_route(&routes, host), expected, "{host}");
        }
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
