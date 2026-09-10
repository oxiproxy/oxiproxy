//! Shared download routing for command-line self updates.
use anyhow::{bail, Context, Result};
use clap::{Args, ValueEnum};
use reqwest::blocking::Client;
use std::{
    fs::File,
    io::{Read, Write},
    path::Path,
    time::{Duration, Instant},
};

const DEFAULT_MIRROR: &str = "https://gh-proxy.com";

#[derive(Clone, Copy, Debug, Default, ValueEnum, PartialEq, Eq)]
pub enum DownloadSource {
    /// 测速后优先使用最快线路，失败时切换
    #[default]
    Auto,
    /// 仅使用 GitHub 直连
    Direct,
    /// 仅使用镜像（未配置时使用默认镜像，多个镜像会测速排序）
    Mirror,
}

#[derive(Args, Debug, Default)]
pub struct UpdateOptions {
    /// 更新包下载线路
    #[arg(
        long,
        value_enum,
        default_value = "auto",
        env = "OXIPROXY_UPDATE_SOURCE"
    )]
    pub source: DownloadSource,
    /// 自定义镜像，替换默认 gh-proxy.com；可重复指定，环境变量用逗号分隔
    #[arg(long, env = "OXIPROXY_UPDATE_MIRRORS", value_delimiter = ',')]
    pub mirror: Vec<String>,
}

impl UpdateOptions {
    pub fn validate(&self) -> Result<()> {
        if self.mirror.len() > 8 {
            bail!("最多配置 8 条镜像线路");
        }
        for mirror in &self.mirror {
            let url = reqwest::Url::parse(mirror).context("镜像必须是完整的 HTTPS URL")?;
            if url.scheme() != "https"
                || url.host_str().is_none()
                || !url.username().is_empty()
                || url.password().is_some()
                || url.query().is_some()
                || url.fragment().is_some()
            {
                bail!("镜像必须是无凭据、查询参数和片段的 HTTPS URL 前缀");
            }
        }
        Ok(())
    }

    fn routes(&self, original: &str) -> Vec<(String, String)> {
        let mut routes = Vec::new();
        if self.source != DownloadSource::Mirror {
            routes.push(("GitHub 直连".into(), original.into()));
        }
        if self.source != DownloadSource::Direct {
            let mirrors: Vec<&str> = if self.mirror.is_empty() {
                vec![DEFAULT_MIRROR]
            } else {
                self.mirror.iter().map(String::as_str).collect()
            };
            for mirror in mirrors {
                let url = format!("{}/{}", mirror.trim_end_matches('/'), original);
                if !routes.iter().any(|(_, existing)| existing == &url) {
                    routes.push((format!("镜像 {}", mirror), url));
                }
            }
        }
        routes
    }

    pub fn download(&self, original: &str, destination: &Path) -> Result<()> {
        self.validate()?;
        let client = Client::builder()
            .user_agent("oxiproxy-updater")
            .connect_timeout(Duration::from_secs(5))
            .timeout(Duration::from_secs(300))
            .build()?;
        let mut routes = self.routes(original);
        if routes.len() > 1 {
            println!(
                "正在并行测速 {} 条下载线路（每条最多 5 秒）...",
                routes.len()
            );
            let scores = std::thread::scope(|scope| {
                let handles: Vec<_> = routes
                    .iter()
                    .map(|(_, url)| {
                        let client = &client;
                        scope.spawn(move || probe(client, url).unwrap_or(0.0))
                    })
                    .collect();
                handles
                    .into_iter()
                    .map(|h| h.join().unwrap_or(0.0))
                    .collect::<Vec<_>>()
            });
            let mut ranked: Vec<_> = routes.into_iter().zip(scores).collect();
            for ((name, _), speed) in &ranked {
                println!(
                    "  {}: {:.1} KiB/s{}",
                    name,
                    speed / 1024.0,
                    if *speed == 0.0 {
                        "（测速失败，保留为备用）"
                    } else {
                        ""
                    }
                );
            }
            ranked.sort_by(|a, b| b.1.total_cmp(&a.1));
            routes = ranked.into_iter().map(|(route, _)| route).collect();
        }
        download_routes(&client, &routes, destination)
    }
}

fn probe(client: &Client, url: &str) -> Result<f64> {
    let start = Instant::now();
    let response = client
        .get(url)
        .header(reqwest::header::RANGE, "bytes=0-262143")
        .timeout(Duration::from_secs(5))
        .send()?
        .error_for_status()?;
    reject_html(&response)?;
    let bytes = std::io::copy(&mut response.take(256 * 1024), &mut std::io::sink())?;
    Ok(bytes as f64 / start.elapsed().as_secs_f64().max(0.001))
}

fn reject_html(response: &reqwest::blocking::Response) -> Result<()> {
    if response
        .headers()
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .is_some_and(|v| v.starts_with("text/html"))
    {
        bail!("下载线路返回了 HTML 页面");
    }
    Ok(())
}

fn download_routes(client: &Client, routes: &[(String, String)], destination: &Path) -> Result<()> {
    for (name, url) in routes {
        println!("使用线路: {}", name);
        match download_one(client, url, destination) {
            Ok(()) => return Ok(()),
            Err(_) => eprintln!("{} 下载失败，尝试下一条线路", name),
        }
    }
    let _ = std::fs::remove_file(destination);
    bail!("所有更新下载线路均失败，请检查网络或更换 --mirror（每条线路超时 300 秒）")
}

fn download_one(client: &Client, url: &str, destination: &Path) -> Result<()> {
    let mut response = client
        .get(url)
        .header(reqwest::header::ACCEPT, "application/octet-stream")
        .send()?
        .error_for_status()?;
    reject_html(&response)?;
    if response.status() != reqwest::StatusCode::OK {
        bail!("下载线路未返回完整文件");
    }
    let expected = response.content_length();
    let mut file = File::create(destination)?;
    let mut buffer = [0u8; 32 * 1024];
    let mut total = 0u64;
    let start = Instant::now();
    let mut last = start;
    loop {
        let bytes = response.read(&mut buffer)?;
        if bytes == 0 {
            break;
        }
        file.write_all(&buffer[..bytes])?;
        total += bytes as u64;
        if last.elapsed() >= Duration::from_secs(1) {
            eprint!(
                "\r已下载 {:.2} MiB，平均 {:.1} KiB/s",
                total as f64 / 1048576.0,
                total as f64 / 1024.0 / start.elapsed().as_secs_f64()
            );
            last = Instant::now();
        }
    }
    eprintln!();
    if total == 0 || expected.is_some_and(|length| length != total) {
        bail!("下载内容为空或不完整");
    }
    file.flush()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::TcpListener;

    #[test]
    fn parses_update_arguments() {
        use clap::Parser;
        #[derive(Parser)]
        struct Cli {
            #[command(flatten)]
            options: UpdateOptions,
        }
        let cli = Cli::try_parse_from([
            "update",
            "--source",
            "mirror",
            "--mirror",
            "https://a.example",
            "--mirror",
            "https://b.example",
        ])
        .unwrap();
        assert_eq!(cli.options.source, DownloadSource::Mirror);
        assert_eq!(cli.options.mirror.len(), 2);
        assert!(Cli::try_parse_from(["update", "--source", "unknown"]).is_err());
    }

    #[test]
    fn validates_mirrors_and_source() {
        for url in [
            "http://example.com",
            "https://user:pass@example.com",
            "https://example.com/?token=x",
            "https://example.com/#x",
        ] {
            assert!(UpdateOptions {
                source: DownloadSource::Auto,
                mirror: vec![url.into()]
            }
            .validate()
            .is_err());
        }
        assert!(UpdateOptions {
            source: DownloadSource::Mirror,
            mirror: vec![]
        }
        .validate()
        .is_ok());
        assert!(UpdateOptions {
            source: DownloadSource::Auto,
            mirror: vec!["https://example.com/proxy/".into()]
        }
        .validate()
        .is_ok());
    }

    #[test]
    fn default_mirror_is_used_unless_overridden_or_direct() {
        let original = "https://github.com/org/repo/releases/download/v1/app.tar.gz";
        let mut options = UpdateOptions::default();
        let routes = options.routes(original);
        assert_eq!(routes.len(), 2);
        assert_eq!(routes[0].1, original);
        assert_eq!(routes[1].1, format!("{DEFAULT_MIRROR}/{original}"));
        options.source = DownloadSource::Mirror;
        assert!(options.validate().is_ok());
        assert_eq!(options.routes(original), vec![routes[1].clone()]);
        options.source = DownloadSource::Direct;
        assert_eq!(options.routes(original), vec![routes[0].clone()]);
        options.source = DownloadSource::Auto;
        options.mirror = vec!["https://custom.example".into()];
        let routes = options.routes(original);
        assert_eq!(routes.len(), 2);
        assert_eq!(routes[1].1, format!("https://custom.example/{original}"));
    }

    #[test]
    fn builds_routes_and_honors_explicit_source() {
        let mut options = UpdateOptions {
            source: DownloadSource::Auto,
            mirror: vec!["https://example.com/".into(), "https://example.com".into()],
        };
        let original = "https://github.com/org/repo/releases/download/v1/app.tar.gz";
        let routes = options.routes(original);
        assert_eq!(routes.len(), 2);
        assert_eq!(routes[1].1, format!("https://example.com/{original}"));
        options.source = DownloadSource::Direct;
        assert_eq!(options.routes(original).len(), 1);
        assert_eq!(options.routes(original)[0].1, original);
        options.source = DownloadSource::Mirror;
        assert_eq!(options.routes(original).len(), 1);
        assert_ne!(options.routes(original)[0].1, original);
    }

    fn serve(responses: Vec<&'static str>) -> (String, std::thread::JoinHandle<()>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        let handle = std::thread::spawn(move || {
            for response in responses {
                let (mut stream, _) = listener.accept().unwrap();
                stream
                    .set_read_timeout(Some(Duration::from_secs(5)))
                    .unwrap();
                let mut request = [0; 4096];
                let _ = stream.read(&mut request).unwrap();
                stream.write_all(response.as_bytes()).unwrap();
            }
        });
        (url, handle)
    }

    #[test]
    fn failed_partial_download_is_replaced_by_fallback() {
        let (url, server) = serve(vec![
            "HTTP/1.1 200 OK\r\nContent-Length: 100\r\nConnection: close\r\n\r\npartial",
            "HTTP/1.1 200 OK\r\nContent-Length: 4\r\nConnection: close\r\n\r\ngood",
        ]);
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("archive");
        let client = Client::builder()
            .no_proxy()
            .timeout(Duration::from_secs(5))
            .build()
            .unwrap();
        download_routes(
            &client,
            &[("first".into(), url.clone()), ("second".into(), url)],
            &path,
        )
        .unwrap();
        assert_eq!(std::fs::read(path).unwrap(), b"good");
        server.join().unwrap();
    }

    #[test]
    fn html_is_rejected_and_failed_archive_removed() {
        let (url, server) = serve(vec!["HTTP/1.1 200 OK\r\nContent-Type: text/html\r\nContent-Length: 4\r\nConnection: close\r\n\r\noops"]);
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("archive");
        std::fs::write(&path, "stale").unwrap();
        let client = Client::builder().no_proxy().build().unwrap();
        assert!(download_routes(&client, &[("mirror".into(), url)], &path).is_err());
        assert!(!path.exists());
        server.join().unwrap();
    }

    #[test]
    fn probe_measures_body_even_without_range_support() {
        let (url, server) = serve(vec![
            "HTTP/1.1 200 OK\r\nContent-Length: 4\r\nConnection: close\r\n\r\ndata",
        ]);
        assert!(probe(&Client::builder().no_proxy().build().unwrap(), &url).unwrap() > 0.0);
        server.join().unwrap();
    }
}
