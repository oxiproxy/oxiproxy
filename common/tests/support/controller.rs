//! An actual gRPC Controller transport with test-controlled responses.
#![allow(dead_code)]

use common::grpc::{
    oxiproxy, AgentClientService, AgentClientServiceServer, AgentServerService,
    AgentServerServiceServer,
};
use tokio::sync::mpsc;
use tokio_stream::wrappers::{ReceiverStream, TcpListenerStream};
use tonic::{Request, Response, Status, Streaming};

pub type ClientSession = (
    Streaming<oxiproxy::AgentClientMessage>,
    mpsc::Sender<Result<oxiproxy::ControllerToClientMessage, Status>>,
);
pub type NodeSession = (
    Streaming<oxiproxy::AgentServerMessage>,
    mpsc::Sender<Result<oxiproxy::ControllerToAgentMessage, Status>>,
);

pub struct Controller {
    pub url: String,
    pub clients: mpsc::UnboundedReceiver<ClientSession>,
    pub nodes: mpsc::UnboundedReceiver<NodeSession>,
    handle: tokio::task::JoinHandle<()>,
}

#[derive(Clone)]
struct Service {
    clients: mpsc::UnboundedSender<ClientSession>,
    nodes: mpsc::UnboundedSender<NodeSession>,
}

impl Controller {
    pub async fn start() -> Self {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        let (client_tx, clients) = mpsc::unbounded_channel();
        let (node_tx, nodes) = mpsc::unbounded_channel();
        let service = Service {
            clients: client_tx,
            nodes: node_tx,
        };
        let handle = tokio::spawn(async move {
            tonic::transport::Server::builder()
                .add_service(AgentClientServiceServer::new(service.clone()))
                .add_service(AgentServerServiceServer::new(service))
                .serve_with_incoming(TcpListenerStream::new(listener))
                .await
                .unwrap();
        });
        Self {
            url,
            clients,
            nodes,
            handle,
        }
    }
}

impl Drop for Controller {
    fn drop(&mut self) {
        self.handle.abort();
    }
}

#[tonic::async_trait]
impl AgentClientService for Service {
    type AgentClientChannelStream =
        ReceiverStream<Result<oxiproxy::ControllerToClientMessage, Status>>;

    async fn agent_client_channel(
        &self,
        request: Request<Streaming<oxiproxy::AgentClientMessage>>,
    ) -> Result<Response<Self::AgentClientChannelStream>, Status> {
        let (tx, rx) = mpsc::channel(16);
        self.clients.send((request.into_inner(), tx)).unwrap();
        Ok(Response::new(ReceiverStream::new(rx)))
    }
}

#[tonic::async_trait]
impl AgentServerService for Service {
    type AgentServerChannelStream =
        ReceiverStream<Result<oxiproxy::ControllerToAgentMessage, Status>>;

    async fn agent_server_channel(
        &self,
        request: Request<Streaming<oxiproxy::AgentServerMessage>>,
    ) -> Result<Response<Self::AgentServerChannelStream>, Status> {
        let (tx, rx) = mpsc::channel(16);
        self.nodes.send((request.into_inner(), tx)).unwrap();
        Ok(Response::new(ReceiverStream::new(rx)))
    }
}
