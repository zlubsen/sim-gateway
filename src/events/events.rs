use std::net::SocketAddr;
use crate::model::config::Scheme;

#[derive(Debug, Clone)]
pub enum Event {
    Message(String),
    Error(String),
    SocketConnected(Scheme, SocketAddr),
    SocketDisconnected(Scheme, SocketAddr),
    StatBytesReceived(usize, usize), // route ID and #bytes
    StatBytesSend(usize, usize), // route ID and #bytes
    ErrorTooManyConnections(usize, String),
}