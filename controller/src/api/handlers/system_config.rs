use axum::{
    extract::Extension,
    response::Json,
};
use sea_orm::{EntityTrait, Set, ActiveModelTrait, ColumnTrait, QueryFilter};
use serde::{Deserialize, Serialize};
use crate::entity::{SystemConfig, system_config};
use crate::migration::get_connection;
use crate::AppState;
use super::ApiResponse;
use crate::middleware::AuthUser;

#[derive(Debug, Serialize)]
pub struct ConfigListResponse {
    pub configs: Vec<ConfigItem>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ConfigItem {
    pub id: i64,
    pub key: String,
    pub value: serde_json::Value,
    pub description: String,
    #[serde(rename = "valueType")]
    pub value_type: String,
}

#[derive(Debug, Deserialize)]
pub struct UpdateConfigRequest {
    pub key: String,
    pub value: serde_json::Value,
}

/// 获取所有系统配置（仅管理员）
pub async fn get_configs(
    Extension(auth_user_opt): Extension<Option<AuthUser>>,
) -> Json<ApiResponse<ConfigListResponse>> {
    let auth_user = match auth_user_opt {
        Some(user) => user,
        None => return ApiResponse::error("未登录".to_string()),
    };
    if !auth_user.is_admin {
        return ApiResponse::error("需要管理员权限".to_string());
    }

    let db = get_connection().await;

    match SystemConfig::find().all(db).await {
        Ok(configs) => {
            let items: Vec<ConfigItem> = configs
                .into_iter()
                .map(|c| {
                    let value = serde_json::from_str(&c.value).unwrap_or(serde_json::Value::Null);
                    ConfigItem {
                        id: c.id,
                        key: c.key,
                        value,
                        description: c.description,
                        value_type: c.value_type,
                    }
                })
                .collect();

            ApiResponse::success(ConfigListResponse { configs: items })
        }
        Err(e) => ApiResponse::error(format!("获取配置失败: {}", e)),
    }
}

/// 更新系统配置（仅管理员）
pub async fn update_config(
    Extension(auth_user_opt): Extension<Option<AuthUser>>,
    Extension(app_state): Extension<AppState>,
    Json(payload): Json<UpdateConfigRequest>,
) -> Json<ApiResponse<ConfigItem>> {
    let auth_user = match auth_user_opt {
        Some(user) => user,
        None => return ApiResponse::error("未登录".to_string()),
    };
    if !auth_user.is_admin {
        return ApiResponse::error("需要管理员权限".to_string());
    }

    let config_manager = &app_state.config_manager;
    let db = get_connection().await;

    // 查找配置
    let config = match SystemConfig::find()
        .filter(system_config::Column::Key.eq(&payload.key))
        .one(db)
        .await
    {
        Ok(Some(c)) => c,
        Ok(None) => {
            return ApiResponse::error(format!("配置项不存在: {}", payload.key));
        }
        Err(e) => {
            return ApiResponse::error(format!("查询配置失败: {}", e));
        }
    };

    // 验证并转换值
    let value_str = match config.value_type.as_str() {
        "number" => {
            if let Some(n) = payload.value.as_i64() {
                n.to_string()
            } else if let Some(f) = payload.value.as_f64() {
                f.to_string()
            } else {
                return ApiResponse::error("配置值类型错误：需要数字类型".to_string());
            }
        }
        "boolean" => {
            if let Some(b) = payload.value.as_bool() {
                b.to_string()
            } else {
                return ApiResponse::error("配置值类型错误：需要布尔类型".to_string());
            }
        }
        "string" => {
            if let Some(s) = payload.value.as_str() {
                serde_json::to_string(s).unwrap_or_else(|_| s.to_string())
            } else {
                return ApiResponse::error("配置值类型错误：需要字符串类型".to_string());
            }
        }
        _ => payload.value.to_string(),
    };

    // 更新数据库
    let mut active_model: system_config::ActiveModel = config.clone().into();
    active_model.value = Set(value_str);
    active_model.updated_at = Set(chrono::Utc::now().naive_utc());

    match active_model.update(db).await {
        Ok(updated) => {
            // 重新加载配置缓存
            if let Err(e) = config_manager.reload().await {
                tracing::error!("重新加载配置缓存失败: {}", e);
            }

            let value = serde_json::from_str(&updated.value).unwrap_or(serde_json::Value::Null);
            ApiResponse::success(ConfigItem {
                id: updated.id,
                key: updated.key,
                value,
                description: updated.description,
                value_type: updated.value_type,
            })
        }
        Err(e) => ApiResponse::error(format!("更新配置失败: {}", e)),
    }
}

/// 批量更新配置
#[derive(Debug, Deserialize)]
pub struct BatchUpdateConfigRequest {
    pub configs: Vec<UpdateConfigRequest>,
}

pub async fn batch_update_configs(
    Extension(auth_user_opt): Extension<Option<AuthUser>>,
    Extension(app_state): Extension<AppState>,
    Json(payload): Json<BatchUpdateConfigRequest>,
) -> Json<ApiResponse<ConfigListResponse>> {
    let auth_user = match auth_user_opt {
        Some(user) => user,
        None => return ApiResponse::error("未登录".to_string()),
    };
    if !auth_user.is_admin {
        return ApiResponse::error("需要管理员权限".to_string());
    }

    let config_manager = &app_state.config_manager;
    let db = get_connection().await;
    let mut updated_items = Vec::new();

    for update_req in payload.configs {
        // 查找配置
        let config = match SystemConfig::find()
            .filter(system_config::Column::Key.eq(&update_req.key))
            .one(db)
            .await
        {
            Ok(Some(c)) => c,
            Ok(None) => continue,
            Err(_) => continue,
        };

        // 验证并转换值
        let value_str = match config.value_type.as_str() {
            "number" => {
                if let Some(n) = update_req.value.as_i64() {
                    n.to_string()
                } else if let Some(f) = update_req.value.as_f64() {
                    f.to_string()
                } else {
                    continue;
                }
            }
            "boolean" => {
                if let Some(b) = update_req.value.as_bool() {
                    b.to_string()
                } else {
                    continue;
                }
            }
            "string" => {
                if let Some(s) = update_req.value.as_str() {
                    serde_json::to_string(s).unwrap_or_else(|_| s.to_string())
                } else {
                    continue;
                }
            }
            _ => update_req.value.to_string(),
        };

        // 更新数据库
        let mut active_model: system_config::ActiveModel = config.into();
        active_model.value = Set(value_str);
        active_model.updated_at = Set(chrono::Utc::now().naive_utc());

        if let Ok(updated) = active_model.update(db).await {
            let value = serde_json::from_str(&updated.value).unwrap_or(serde_json::Value::Null);
            updated_items.push(ConfigItem {
                id: updated.id,
                key: updated.key,
                value,
                description: updated.description,
                value_type: updated.value_type,
            });
        }
    }

    // 重新加载配置缓存
    if let Err(e) = config_manager.reload().await {
        tracing::error!("重新加载配置缓存失败: {}", e);
    }

    ApiResponse::success(ConfigListResponse { configs: updated_items })
}

/// 重启系统响应
#[derive(Debug, Serialize)]
pub struct RestartResponse {
    pub message: String,
}

/// 重启系统（仅管理员可用）
pub async fn restart_system(
    Extension(auth_user): Extension<Option<AuthUser>>,
) -> Json<ApiResponse<RestartResponse>> {
    // 检查是否已登录
    let auth_user = match auth_user {
        Some(user) => user,
        None => {
            return ApiResponse::error("未登录，请先登录".to_string());
        }
    };

    // 检查是否为管理员
    if !auth_user.is_admin {
        return ApiResponse::error("权限不足，仅管理员可以重启系统".to_string());
    }

    tracing::info!("管理员 {} 请求重启系统", auth_user.username);

    // 获取当前可执行文件路径
    let exe_path = match std::env::current_exe() {
        Ok(path) => path,
        Err(e) => {
            tracing::error!("无法获取可执行文件路径: {}", e);
            return ApiResponse::error("无法获取可执行文件路径".to_string());
        }
    };

    // 获取当前工作目录
    let current_dir = std::env::current_dir().ok();

    // 在后台线程中延迟重启
    tokio::spawn(async move {
        // 等待 300ms 让响应先发送
        tokio::time::sleep(tokio::time::Duration::from_millis(300)).await;
        tracing::info!("系统正在重启...");

        // 构建启动命令，使用延迟启动让当前进程有时间退出并释放端口
        #[cfg(windows)]
        {
            // Windows: 使用 cmd /c 延迟启动
            let exe_path_str = exe_path.to_string_lossy().to_string();
            let mut cmd = std::process::Command::new("cmd");
            cmd.args(["/c", "timeout", "/t", "2", "/nobreak", ">nul", "&&", &exe_path_str]);
            if let Some(dir) = current_dir {
                cmd.current_dir(dir);
            }
            match cmd.spawn() {
                Ok(_) => {
                    tracing::info!("重启命令已发送，当前进程即将退出");
                }
                Err(e) => {
                    tracing::error!("启动重启命令失败: {}", e);
                }
            }
        }

        #[cfg(not(windows))]
        {
            // Linux/Unix: 使用 sh -c 延迟启动
            let exe_path_str = exe_path.to_string_lossy().to_string();
            let mut cmd = std::process::Command::new("sh");
            cmd.args(["-c", &format!("sleep 2 && {}", exe_path_str)]);
            if let Some(dir) = current_dir {
                cmd.current_dir(dir);
            }
            match cmd.spawn() {
                Ok(_) => {
                    tracing::info!("重启命令已发送，当前进程即将退出");
                }
                Err(e) => {
                    tracing::error!("启动重启命令失败: {}", e);
                }
            }
        }

        // 退出当前进程
        std::process::exit(0);
    });

    ApiResponse::success(RestartResponse {
        message: "系统将在 2 秒后重启".to_string(),
    })
}
