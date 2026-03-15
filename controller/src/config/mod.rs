//! Controller 配置模块

use serde::{Deserialize, Serialize};
use std::fs;
use std::path::Path;
use tokio::sync::OnceCell;

/// Controller 配置
#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct Config {
    /// Web 管理界面端口
    #[serde(default = "default_web_port")]
    pub web_port: u16,

    /// 内部 API 端口（供节点调用）
    #[serde(default = "default_internal_port")]
    pub internal_port: u16,

    /// JWT 密钥 (可选，默认从环境变量 JWT_SECRET 读取)
    #[serde(default)]
    pub jwt_secret: Option<String>,

    /// JWT 过期时间（小时）
    #[serde(default = "default_jwt_expiration")]
    pub jwt_expiration_hours: i64,

    /// 数据库路径
    #[serde(default = "default_db_path")]
    pub db_path: String,

    /// 节点→Controller 通信的内部密钥
    #[serde(default)]
    pub internal_secret: Option<String>,

    /// (向后兼容) frps 内部 API 地址
    #[serde(default)]
    pub frps_url: Option<String>,

    /// (向后兼容) frps 内部 API 共享密钥
    #[serde(default)]
    pub frps_secret: Option<String>,
}

fn default_web_port() -> u16 {
    3000
}

fn default_internal_port() -> u16 {
    3100
}

fn default_jwt_expiration() -> i64 {
    24
}

fn default_db_path() -> String {
    "./data/oxiproxy.db".to_string()
}

impl Config {
    /// 获取内部 API 密钥（优先 internal_secret，回退 frps_secret）
    pub fn get_internal_secret(&self) -> String {
        if let Some(ref secret) = self.internal_secret {
            if !secret.is_empty() {
                return secret.clone();
            }
        }
        self.frps_secret.clone().unwrap_or_default()
    }

    /// 获取 JWT 密钥（优先从环境变量读取，其次从配置文件，最后自动生成）
    pub fn get_jwt_secret(&self) -> anyhow::Result<String> {
        // 优先从环境变量读取
        if let Ok(secret) = std::env::var("JWT_SECRET") {
            if !secret.is_empty() {
                return Ok(secret);
            }
        }

        // 其次从配置文件读取
        if let Some(ref secret) = self.jwt_secret {
            if !secret.is_empty() {
                return Ok(secret.clone());
            }
        }

        // 如果都没有，从持久化文件读取或生成新密钥
        Self::get_or_generate_jwt_secret()
    }

    /// 从文件获取或生成新的 JWT 密钥
    fn get_or_generate_jwt_secret() -> anyhow::Result<String> {
        use std::path::PathBuf;

        let data_dir = PathBuf::from("./data");
        let secret_file = data_dir.join("jwt_secret.key");

        // 尝试从文件读取
        if secret_file.exists() {
            if let Ok(secret) = fs::read_to_string(&secret_file) {
                let secret = secret.trim();
                if !secret.is_empty() {
                    return Ok(secret.to_string());
                }
            }
        }

        // 文件不存在或读取失败，生成新密钥
        let secret = Self::generate_random_secret(64);

        // 确保 data 目录存在
        if let Err(e) = fs::create_dir_all(&data_dir) {
            tracing::warn!("无法创建 data 目录: {}", e);
        } else {
            // 保存密钥到文件
            if let Err(e) = fs::write(&secret_file, &secret) {
                tracing::warn!("无法保存 JWT 密钥到文件: {}", e);
            } else {
                tracing::info!("🔑 已生成并保存新的 JWT 密钥到: {}", secret_file.display());
            }
        }

        Ok(secret)
    }

    /// 生成随机密钥
    fn generate_random_secret(length: usize) -> String {
        use rand::Rng;
        const CHARSET: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/=";
        let mut rng = rand::rng();
        (0..length)
            .map(|_| {
                let idx = rng.gen_range(0..CHARSET.len());
                CHARSET[idx] as char
            })
            .collect()
    }
}

static CONFIG: OnceCell<Config> = OnceCell::const_new();

/// 获取全局配置
pub async fn get_config() -> &'static Config {
    CONFIG.get_or_init(init_config).await
}

/// 初始化配置
pub async fn init_config() -> Config {
    use crate::entity::{SystemConfig, system_config};
    use sea_orm::{EntityTrait, ColumnTrait, QueryFilter};

    // 尝试从数据库读取配置
    if let Ok(db) = crate::migration::get_connection().await.try_into() {
        let db: &sea_orm::DatabaseConnection = db;

        // 读取所有配置项
        if let Ok(configs) = SystemConfig::find().all(db).await {
            let mut config = Config {
                web_port: default_web_port(),
                internal_port: default_internal_port(),
                jwt_secret: None,
                jwt_expiration_hours: default_jwt_expiration(),
                db_path: default_db_path(),
                internal_secret: None,
                frps_url: None,
                frps_secret: None,
            };

            // 从数据库配置项中填充
            for item in configs {
                match item.key.as_str() {
                    "web_port" => {
                        if let Ok(port) = item.value.parse::<u16>() {
                            config.web_port = port;
                        }
                    }
                    "internal_port" => {
                        if let Ok(port) = item.value.parse::<u16>() {
                            config.internal_port = port;
                        }
                    }
                    "jwt_expiration_hours" => {
                        if let Ok(hours) = item.value.parse::<i64>() {
                            config.jwt_expiration_hours = hours;
                        }
                    }
                    "db_path" => {
                        if let Ok(path) = serde_json::from_str::<String>(&item.value) {
                            config.db_path = path;
                        }
                    }
                    _ => {}
                }
            }

            tracing::info!("📋 从数据库加载配置");
            return config;
        }
    }

    // 如果数据库读取失败，尝试从配置文件读取（向后兼容）
    let config_paths = [
        "controller.toml",
        "../controller.toml",
    ];

    for path_str in &config_paths {
        let path = Path::new(path_str);
        if path.exists() {
            let content = fs::read_to_string(path)
                .unwrap_or_else(|e| panic!("无法读取配置文件 {}: {}", path.display(), e));

            let config: Config = toml::from_str(&content)
                .unwrap_or_else(|e| panic!("解析配置文件 {} 失败: {}", path.display(), e));

            tracing::info!("📋 加载配置文件: {}", path.display());
            return config;
        }
    }

    tracing::warn!("未找到配置文件或数据库配置，使用默认配置");
    Config {
        web_port: default_web_port(),
        internal_port: default_internal_port(),
        jwt_secret: None,
        jwt_expiration_hours: default_jwt_expiration(),
        db_path: default_db_path(),
        internal_secret: None,
        frps_url: None,
        frps_secret: None,
    }
}
