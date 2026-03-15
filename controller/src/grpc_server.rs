//! gRPC Server 启动
//!
//! 在 internal_port 上启动 gRPC Server，提供 AgentServerService 和 AgentClientService。
//! 支持原生 TLS（从数据库或文件加载证书）。

use std::sync::Arc;
use tonic::transport::{Identity, Server, ServerTlsConfig};
use tracing::{info, error, warn};
use base64::Engine;

use common::grpc::{AgentServerServiceServer, AgentClientServiceServer};

use crate::grpc_agent_server_service::AgentServerServiceImpl;
use crate::grpc_agent_client_service::AgentClientServiceImpl;
use crate::node_manager::NodeManager;
use crate::client_stream_manager::ClientStreamManager;
use crate::config_manager::ConfigManager;

/// 从 ConfigManager 加载 TLS 证书和私钥（PEM 格式）
async fn load_tls_identity(config_manager: &ConfigManager) -> Result<Identity, String> {
    // 优先从数据库内容读取（base64 编码的 PEM）
    let cert_content = config_manager.get_string("grpc_tls_cert_content", "").await;
    let key_content = config_manager.get_string("grpc_tls_key_content", "").await;

    if !cert_content.is_empty() && !key_content.is_empty() {
        let cert_pem = base64::engine::general_purpose::STANDARD
            .decode(&cert_content)
            .map_err(|e| format!("证书 base64 解码失败: {}", e))?;
        let key_pem = base64::engine::general_purpose::STANDARD
            .decode(&key_content)
            .map_err(|e| format!("私钥 base64 解码失败: {}", e))?;
        info!("从数据库加载 TLS 证书");
        return Ok(Identity::from_pem(cert_pem, key_pem));
    }

    // 回退到文件路径
    let cert_path = config_manager.get_string("grpc_tls_cert_path", "").await;
    let key_path = config_manager.get_string("grpc_tls_key_path", "").await;

    if cert_path.is_empty() || key_path.is_empty() {
        return Err("TLS 已启用但未配置证书：请设置 grpc_tls_cert_content/grpc_tls_key_content 或 grpc_tls_cert_path/grpc_tls_key_path".to_string());
    }

    let cert_pem = tokio::fs::read(&cert_path).await
        .map_err(|e| format!("读取证书文件 {} 失败: {}", cert_path, e))?;
    let key_pem = tokio::fs::read(&key_path).await
        .map_err(|e| format!("读取私钥文件 {} 失败: {}", key_path, e))?;
    info!("从文件加载 TLS 证书: {}", cert_path);
    Ok(Identity::from_pem(cert_pem, key_pem))
}

/// 启动 gRPC Server
pub fn start_grpc_server(
    port: u16,
    node_manager: Arc<NodeManager>,
    client_stream_manager: Arc<ClientStreamManager>,
    config_manager: Arc<ConfigManager>,
) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        let addr = format!("0.0.0.0:{}", port).parse().expect("invalid gRPC listen address");

        let agent_server_service = AgentServerServiceImpl {
            node_manager,
        };

        let agent_client_service = AgentClientServiceImpl {
            client_stream_manager,
        };

        let tls_enabled = config_manager.get_bool("grpc_tls_enabled", false).await;

        if tls_enabled {
            match load_tls_identity(&config_manager).await {
                Ok(identity) => {
                    let tls_config = ServerTlsConfig::new().identity(identity);
                    info!("gRPC Server 启动 (TLS): {}", addr);

                    let mut builder = match Server::builder().tls_config(tls_config) {
                        Ok(b) => b,
                        Err(e) => {
                            error!("gRPC TLS 配置失败: {}，回退到非 TLS 模式", e);
                            warn!("gRPC Server 启动 (非 TLS): {}", addr);
                            if let Err(e) = Server::builder()
                                .add_service(AgentServerServiceServer::new(agent_server_service))
                                .add_service(AgentClientServiceServer::new(agent_client_service))
                                .serve(addr)
                                .await
                            {
                                error!("gRPC Server 错误: {}", e);
                            }
                            return;
                        }
                    };

                    if let Err(e) = builder
                        .add_service(AgentServerServiceServer::new(agent_server_service))
                        .add_service(AgentClientServiceServer::new(agent_client_service))
                        .serve(addr)
                        .await
                    {
                        error!("gRPC Server 错误: {}", e);
                    }
                }
                Err(e) => {
                    error!("加载 TLS 证书失败: {}，回退到非 TLS 模式", e);
                    warn!("gRPC Server 启动 (非 TLS): {}", addr);
                    if let Err(e) = Server::builder()
                        .add_service(AgentServerServiceServer::new(agent_server_service))
                        .add_service(AgentClientServiceServer::new(agent_client_service))
                        .serve(addr)
                        .await
                    {
                        error!("gRPC Server 错误: {}", e);
                    }
                }
            }
        } else {
            info!("gRPC Server 启动: {}", addr);

            if let Err(e) = Server::builder()
                .add_service(AgentServerServiceServer::new(agent_server_service))
                .add_service(AgentClientServiceServer::new(agent_client_service))
                .serve(addr)
                .await
            {
                error!("gRPC Server 错误: {}", e);
            }
        }
    })
}
