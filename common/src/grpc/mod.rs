pub mod pending_requests;
pub mod connection;

// 导出 proto 生成的代码
pub mod oxiproxy {
    tonic::include_proto!("oxiproxy");
}

// 重新导出常用类型
pub use oxiproxy::*;
pub use oxiproxy::agent_server_service_client::AgentServerServiceClient;
pub use oxiproxy::agent_server_service_server::{AgentServerService, AgentServerServiceServer};
pub use oxiproxy::agent_client_service_client::AgentClientServiceClient;
pub use oxiproxy::agent_client_service_server::{AgentClientService, AgentClientServiceServer};
