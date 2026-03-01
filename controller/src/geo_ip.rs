//! IP 地理位置查询服务
//!
//! 优先使用 cz88（纯真 IP，国内外准确、原生中文），备选 ip-api.com（支持中文），
//! 最终备选 ip.sb

use anyhow::{anyhow, Result};
use serde::{Deserialize, Serialize};
use tracing::{error, info};

/// IP 地理位置信息
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GeoIpInfo {
    pub ip: String,
    pub region: String,
}

/// cz88 openIPInfo 响应
#[derive(Debug, Deserialize)]
struct Cz88Response {
    code: Option<i32>,
    success: Option<bool>,
    data: Option<Cz88Data>,
}

#[derive(Debug, Deserialize)]
struct Cz88Data {
    ip: Option<String>,
    geo: Option<String>,
}

/// ip-api.com 响应（支持中文 lang=zh-CN）
#[derive(Debug, Deserialize)]
struct IpApiResponse {
    query: Option<String>,
    country: Option<String>,
    #[serde(rename = "regionName")]
    region_name: Option<String>,
    city: Option<String>,
    status: Option<String>,
}

/// ip.sb 响应（备选，返回英文）
#[derive(Debug, Deserialize)]
struct IpSbResponse {
    ip: Option<String>,
    country: Option<String>,
    region: Option<String>,
    city: Option<String>,
}

/// 查询 IP 地址的地理位置信息
/// 优先 cz88（国内外准确、中文），备选 ip-api.com（中文），最终 ip.sb（英文）
pub async fn query_geo_ip(ip: &str) -> Result<GeoIpInfo> {
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(5))
        .build()?;

    // 优先使用 cz88 纯真 IP，国内外都准确，原生中文
    match query_cz88(&client, ip).await {
        Ok(info) => return Ok(info),
        Err(e) => {
            error!("cz88 查询失败，尝试 ip-api.com: {}", e);
        }
    }

    // 备选：ip-api.com，支持中文
    match query_ip_api(&client, ip).await {
        Ok(info) => return Ok(info),
        Err(e) => {
            error!("ip-api.com 查询失败，尝试 ip.sb: {}", e);
        }
    }

    // 最终备选：ip.sb（英文）
    query_ip_sb(&client, ip).await
}

/// 使用 cz88 纯真 IP 查询（国内外准确，原生中文）
async fn query_cz88(client: &reqwest::Client, ip: &str) -> Result<GeoIpInfo> {
    let url = format!(
        "https://www.cz88.net/api/cz88/ip/openIPInfo?ip={}",
        ip
    );

    let response = client
        .get(&url)
        .send()
        .await
        .map_err(|e| anyhow!("请求 cz88 失败: {}", e))?;

    if !response.status().is_success() {
        return Err(anyhow!("cz88 返回错误状态: {}", response.status()));
    }

    let api_response: Cz88Response = response
        .json()
        .await
        .map_err(|e| anyhow!("解析 cz88 响应失败: {}", e))?;

    if api_response.code != Some(200) || api_response.success != Some(true) {
        return Err(anyhow!("cz88 查询失败"));
    }

    let data = api_response.data.ok_or_else(|| anyhow!("cz88 返回数据为空"))?;

    let geo = data
        .geo
        .filter(|s| !s.is_empty())
        .ok_or_else(|| anyhow!("cz88 地区信息为空"))?;

    // cz88 返回格式如 "中国–北京–北京"，用中文短横线分隔，统一替换为普通短横线
    let region = geo.replace('–', "-");

    let ip = data.ip.unwrap_or_else(|| ip.to_string());

    info!("查询到 IP {} 的地理位置: {}", ip, region);
    Ok(GeoIpInfo { ip, region })
}

/// 使用 ip-api.com 查询（返回中文地区名，海外 IP 覆盖好）
async fn query_ip_api(client: &reqwest::Client, ip: &str) -> Result<GeoIpInfo> {
    let url = format!(
        "http://ip-api.com/json/{}?lang=zh-CN&fields=status,query,country,regionName,city",
        ip
    );

    let response = client
        .get(&url)
        .send()
        .await
        .map_err(|e| anyhow!("请求 ip-api.com 失败: {}", e))?;

    if !response.status().is_success() {
        return Err(anyhow!(
            "ip-api.com 返回错误状态: {}",
            response.status()
        ));
    }

    let api_response: IpApiResponse = response
        .json()
        .await
        .map_err(|e| anyhow!("解析 ip-api.com 响应失败: {}", e))?;

    if api_response.status.as_deref() != Some("success") {
        return Err(anyhow!("ip-api.com 查询失败"));
    }

    let region = build_region_string(
        api_response.country,
        api_response.region_name,
        api_response.city,
    );
    let ip = api_response.query.unwrap_or_else(|| ip.to_string());

    info!("查询到 IP {} 的地理位置: {}", ip, region);
    Ok(GeoIpInfo { ip, region })
}

/// 使用 ip.sb 查询（最终备选，返回英文）
async fn query_ip_sb(client: &reqwest::Client, ip: &str) -> Result<GeoIpInfo> {
    let url = format!("https://api.ip.sb/geoip/{}", ip);

    let response = client
        .get(&url)
        .header("User-Agent", "Mozilla/5.0")
        .send()
        .await
        .map_err(|e| anyhow!("请求 ip.sb 失败: {}", e))?;

    if !response.status().is_success() {
        return Err(anyhow!("ip.sb 返回错误状态: {}", response.status()));
    }

    let api_response: IpSbResponse = response
        .json()
        .await
        .map_err(|e| anyhow!("解析 ip.sb 响应失败: {}", e))?;

    let region = build_region_string(
        api_response.country,
        api_response.region,
        api_response.city,
    );
    let ip = api_response.ip.unwrap_or_else(|| ip.to_string());

    info!("查询到 IP {} 的地理位置（备选服务）: {}", ip, region);
    Ok(GeoIpInfo { ip, region })
}

/// 构建地区字符串：国家-省份-城市（自动去重，用于 ip-api.com / ip.sb）
fn build_region_string(
    country: Option<String>,
    region: Option<String>,
    city: Option<String>,
) -> String {
    let country = country.filter(|s| !s.is_empty());
    let region = region.filter(|s| !s.is_empty());
    let city = city.filter(|s| !s.is_empty());

    let mut parts: Vec<String> = Vec::new();

    if let Some(ref country) = country {
        parts.push(country.clone());
    }

    if let Some(ref region) = region {
        // 避免省份与国家重复
        let stripped = country
            .as_deref()
            .and_then(|c| region.strip_prefix(c))
            .unwrap_or(region.as_str());
        if !stripped.is_empty() {
            parts.push(stripped.to_string());
        }
    }

    if let Some(ref city) = city {
        // 避免城市与省份重复（如 region="东京都", city="东京"）
        let duplicate = parts
            .last()
            .map_or(false, |last| last.contains(city.as_str()) || city.contains(last.as_str()));
        if !duplicate {
            parts.push(city.clone());
        }
    }

    if parts.is_empty() {
        "Unknown".to_string()
    } else {
        parts.join("-")
    }
}

/// 从 gRPC 连接中提取客户端 IP 地址
pub fn extract_client_ip_from_request<T>(request: &tonic::Request<T>) -> Option<String> {
    // 尝试从 metadata 中获取真实 IP（如果有反向代理）
    if let Some(forwarded) = request.metadata().get("x-forwarded-for") {
        if let Ok(forwarded_str) = forwarded.to_str() {
            // X-Forwarded-For 可能包含多个 IP，取第一个
            if let Some(first_ip) = forwarded_str.split(',').next() {
                return Some(first_ip.trim().to_string());
            }
        }
    }

    // 从 remote_addr 获取
    if let Some(remote_addr) = request.remote_addr() {
        return Some(remote_addr.ip().to_string());
    }

    None
}

/// 查询节点的公网 IP 和地理位置
/// 如果提供了 IP 地址，则查询该 IP；否则查询本机公网 IP
pub async fn query_node_geo_info(ip: Option<String>) -> Result<GeoIpInfo> {
    let target_ip = if let Some(ip) = ip {
        ip
    } else {
        // 查询本机公网 IP
        get_public_ip().await?
    };

    query_geo_ip(&target_ip).await
}

/// 获取本机公网 IP 地址
async fn get_public_ip() -> Result<String> {
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(5))
        .build()?;

    // 使用多个服务作为备选
    let services = vec![
        "https://api.ipify.org",
        "https://ifconfig.me/ip",
        "https://icanhazip.com",
    ];

    for service in services {
        match client.get(service).send().await {
            Ok(response) => {
                if response.status().is_success() {
                    if let Ok(ip) = response.text().await {
                        let ip = ip.trim().to_string();
                        if !ip.is_empty() {
                            info!("获取到公网 IP: {}", ip);
                            return Ok(ip);
                        }
                    }
                }
            }
            Err(e) => {
                error!("从 {} 获取公网 IP 失败: {}", service, e);
                continue;
            }
        }
    }

    Err(anyhow!("无法获取公网 IP 地址"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_query_geo_ip() {
        // 测试查询谷歌 DNS 的地理位置
        let result = query_geo_ip("8.8.8.8").await;
        assert!(result.is_ok());
        let info = result.unwrap();
        assert_eq!(info.ip, "8.8.8.8");
        assert!(!info.region.is_empty());
        println!("8.8.8.8 的地理位置: {}", info.region);
    }
}
