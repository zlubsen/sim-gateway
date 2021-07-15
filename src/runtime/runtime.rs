use std::net::{SocketAddr, Ipv4Addr, SocketAddrV4};

use log::{debug, error, info, trace};
use tokio::io::ErrorKind;
use tokio::net::UdpSocket;
use tokio::sync::broadcast::{Receiver, Sender};
use tokio::time::Duration;

use crate::events::{Command, Event};
use crate::model::config::*;
use bytes::BytesMut;
use crate::runtime::RuntimeError::{SendError, ReceiveError};
use std::fmt::{Display, Formatter};
use std::fmt;
use tokio::runtime::Runtime;

pub const DEFAULT_ROUTE_BUFFER_SIZE : usize = 1024;

#[derive(Debug)]
pub enum RuntimeError {
    ReceiveError(String),
    SendError(String),
}

impl std::error::Error for RuntimeError {
}

impl Display for RuntimeError {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match self {
            RuntimeError::ReceiveError(s) => write!(f, "Error receiving: {}", s),
            RuntimeError::SendError(s) => write!(f, "Error sending: {}", s),
        }
    }
}

pub async fn start_runtime_task(mut command_rx: Receiver<Command>, data_tx: Sender<Event>) {
    // spawn gateway main task
    let _main_task = tokio::spawn(start_routes(data_tx));

    loop {
        std::thread::sleep(Duration::from_millis(200));
        let recv = command_rx.try_recv();
        if let Ok(command) = recv {
            match command {
                Command::Quit => {
                    break;
                }
                _ => {}
            }
        }
    }
}

async fn start_routes(data_tx : Sender<Event>) {
    Config::current().routes.iter().for_each(|route| {
        tokio::spawn( start_route(route.clone()));
    });
}

async fn start_route(route : Route) {
    // create separate endpoints, preferable generic
    // receive from the in_point, or both if bidirectional
    // allow for transformations after receiving
    // write to the out_point, or both if bidirectional

    let receiver = UdpReceiver {
        socket : UdpSocket::bind(
            SocketAddrV4::new(Ipv4Addr::UNSPECIFIED,
                              route.in_point.socket.port())
        ).await.expect("Error creating UdpSocket")
    };
    let sender = UdpSender {
        socket : UdpSocket::bind(SocketAddrV4::new(Ipv4Addr::UNSPECIFIED,
                                                   route.out_point.socket.port())
        ).await.expect("Error creating UdpSocket"),
        destination : route.out_point.socket,
    };

    // TODO this is not right yet
    // if route.in_point.scheme == Scheme::UDP && route.out_point.scheme == Scheme::UDP {
    //     create_route_udp_udp(route);
    // } else if route.in_point.scheme == Scheme::UDP && route.out_point.scheme == Scheme::TCP {
    //     create_route_udp_tcp(route);
    // } else if route.in_point.scheme == Scheme::TCP && route.out_point.scheme == Scheme::UDP {
    //     create_route_tcp_udp(route);
    // }
}

async fn create_route_udp_udp(route : &Route) {
    let in_socket = create_udp_socket(&route.in_point).await;
    let out_socket = create_udp_socket(&route.out_point).await;

    // TODO use Bytes crate
    let mut buf = vec![0u8; route.buffer_size];

    loop {
        let (len, addr) = in_socket.recv_from(&mut buf).await.expect("Error receiving from incoming Endpoint.");

        debug!("Received {} bytes from {}.", len, addr);

        match out_socket.send_to(&buf[..len], dst_addr).await {
            Ok(_num_bytes) => {
                break;
            }
            Err(ref err) if err.kind() == ErrorKind::WouldBlock => {
                continue;
            }
            Err(err) => {
                // return Err(e);
                error!("{}", err);
                break;
            }
        }
    }
}

async fn create_route_udp_tcp(route : &Route) {

}

async fn create_route_tcp_udp(route : &Route) {

}

async fn create_udp_socket(endpoint : &EndPoint) -> UdpSocket {
    // bind to incoming port >> 0.0.0.0:port
    let local_addr = SocketAddrV4::new(Ipv4Addr::UNSPECIFIED, endpoint.socket.port());
    let socket = UdpSocket::bind(local_addr).await
        .expect(format!("Error binding Endpoint to local port {}.", local_addr).as_str());
    // set type options
    match endpoint.socket_type {
        SocketType::UdpSocketType(UdpSocketType::Unicast) => {
            socket.set_ttl(endpoint.ttl);
        },
        SocketType::UdpSocketType(UdpSocketType::Broadcast) => {
            socket.set_broadcast(true);
            socket.set_ttl(1); // only broadcast on the local subnet
        },
        SocketType::UdpSocketType(UdpSocketType::Multicast) => {
            socket.join_multicast_v4(endpoint.socket.ip(), Ipv4Addr::UNSPECIFIED);
            socket.set_multicast_ttl_v4(endpoint.ttl);
        },
        _ => {}
    };
    socket
}

trait ReceiverEndPoint {
    fn recv(&self, buf: &mut BytesMut) -> Result<usize, RuntimeError>;
}
trait SenderEndPoint {
    fn send(&self, buf: &mut BytesMut) -> Result<usize, RuntimeError>;
}

struct UdpSender {
    socket : UdpSocket,
    destination : SocketAddr
}

impl SenderEndPoint for UdpSender {
    fn send(&self, buf: &mut BytesMut) -> Result<usize, RuntimeError> {
        match self.socket.send_to(buf, self.destination) {
            Ok(num_bytes) => Ok(num_bytes),
            Err(err) => SendError(format!("Send error: {}", err)),
        }
    }
}

struct UdpReceiver {
    socket: UdpSocket,
}

impl ReceiverEndPoint for UdpReceiver {
    fn recv(&self, buf: &mut BytesMut) -> Result<usize, RuntimeError> {
        match self.socket.recv_from(buf) {
            Ok((num_bytes, _)) => Ok(num_bytes),
            Err(err) => ReceiveError(format!("Receive error: {}", err)),
        }
    }
}