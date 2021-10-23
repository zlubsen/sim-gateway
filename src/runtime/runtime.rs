use std::net::{SocketAddr, Ipv4Addr, SocketAddrV4};

use log::{debug, error, trace};
use tokio::io::ErrorKind;
use tokio::net::{UdpSocket, ToSocketAddrs};
use tokio::sync::broadcast::{Receiver, Sender};
use tokio::time::Duration;

use crate::events::{Command, Event};
use crate::model::config::*;
use bytes::BytesMut;
use crate::runtime::RuntimeError::{SendError, ReceiveError};
use std::fmt::{Display, Formatter};
use std::fmt;

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

    // let _main_task = tokio::spawn( async move {
    //     trace!("tokio.spawn");
    //     start_routes(data_tx).await;
    //     // trace!("finished start_routes");
    // });
    Config::current().routes.iter().for_each(|route| {
        let r = route.clone();
        let r_data_tx = data_tx.clone();
        tokio::spawn( async move {
            start_route(r, r_data_tx).await;
        });
    });

    loop {
        tokio::time::sleep(Duration::from_millis(200)).await;
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

async fn start_route(route : Route, route_data_tx : Sender<Event>) {
    debug!("Spawning route '{}'", route.name);
    // create separate endpoints, preferable generic
    // receive from the in_point, or both if bidirectional
    // allow for transformations after receiving
    // write to the out_point, or both if bidirectional

    match (&route.in_point.scheme, &route.out_point.scheme) {
        (Scheme::UDP, Scheme::UDP) => { create_route_udp_udp(&route, route_data_tx).await; }
        (Scheme::UDP, Scheme::TCP) => { create_route_udp_tcp(&route).await; }
        (Scheme::TCP, Scheme::TCP) => { create_route_tcp_tcp(&route).await; }
        (Scheme::TCP, Scheme::UDP) => { create_route_tcp_udp(&route).await; }
    };
}

async fn create_route_udp_udp(route : &Route, route_data_tx : Sender<Event>) {
    debug!("Creating udp-udp route '{}'", route.name);

    let in_socket = create_udp_socket(&route.in_point).await;
    let out_socket = create_udp_socket(&route.out_point).await;
    let out_address = format!("{}:{}",
                              route.out_point.socket.ip(),
                              route.out_point.socket.port());

    let mut buf = BytesMut::with_capacity(route.buffer_size);
    buf.resize(route.buffer_size, 0);

    loop {
        let (bytes_received, addr) = in_socket.recv_from(&mut buf).await.expect("Error receiving from incoming Endpoint.");
        // collect statistics for rx
        if let Err(msg) = route_data_tx.send(Event::StatBytesReceived(0, bytes_received)) {
            trace!("Error sending runtime receive statistics for route '{}' through channel: {}", route.name, msg);
        } else {
            trace!("StatBytesReceived send through channel");
        }

        trace!("Received {} bytes from {}.", bytes_received, addr);

        match out_socket.send_to(&buf[..bytes_received],
                                 out_address.as_str()).await {
            Ok(bytes_send) => {
                trace!("Successfully send {:?} bytes to {:?} through route '{}'", bytes_send, "192.168.8.158", route.name);
                // collect statistics for tx
                if let Err(msg) = route_data_tx.send(Event::StatBytesSend(0, bytes_send)) {
                    trace!("Error sending runtime send statistics for route '{}' through channel: {}", route.name, msg);
                } else {
                    trace!("StatBytesSend send through channel");
                }
            }
            Err(ref err) if err.kind() == ErrorKind::WouldBlock => {
                continue;
            }
            Err(err) => {
                error!("{}", err);
                break;
            }
        }
    };
}

async fn create_route_udp_tcp(route : &Route) {

}

async fn create_route_tcp_tcp(route : &Route) {

}

async fn create_route_tcp_udp(route : &Route) {

}

async fn create_udp_socket(endpoint : &EndPoint) -> UdpSocket {
    // bind to incoming port >> 0.0.0.0:port
    let local_addr = SocketAddrV4::new(Ipv4Addr::UNSPECIFIED, endpoint.socket.port());
    let socket = UdpSocket::bind(local_addr).await
        .expect(format!("Error binding Endpoint to local port {}.", local_addr).as_str());
    trace!("UDP socket created: {:?}", socket);

    // set type options
    match endpoint.socket_type {
        SocketType::UdpSocketType(UdpSocketType::Unicast) => {
            socket.set_ttl(endpoint.ttl)
                .expect(format!("Error setting TTL for unicast UDP socket {:?}.", endpoint.socket).as_str());
        },
        SocketType::UdpSocketType(UdpSocketType::Broadcast) => {
            socket.set_broadcast(true)
                .expect(format!("Error setting broadcast for UDP socket {:?}.", endpoint.socket).as_str());
            socket.set_ttl(1) // only broadcast on the local subnet
                .expect(format!("Error setting TTL = 1 for broadcast UDP socket {:?}.", endpoint.socket).as_str());
        },
        SocketType::UdpSocketType(UdpSocketType::Multicast) => {
            socket.join_multicast_v4(endpoint.socket.ip().clone(), Ipv4Addr::UNSPECIFIED)
                .expect(format!("Error joining multicast for UDP socket {:?}.", endpoint.socket).as_str());
            socket.set_multicast_ttl_v4(endpoint.ttl)
                .expect(format!("Error setting TTL for multicast UDP socket {:?}.", endpoint.socket).as_str());
        },
        _ => {}
    };
    socket
}

// trait ReceiverEndPoint {
//     fn recv(&self, buf: &mut BytesMut) -> Result<usize, RuntimeError>;
// }
// trait SenderEndPoint {
//     fn send(&self, buf: &mut BytesMut) -> Result<usize, RuntimeError>;
// }
//
// struct UdpSender {
//     socket : UdpSocket,
//     destination : SocketAddr
// }
//
// impl SenderEndPoint for UdpSender {
//     fn send(&self, buf: &mut BytesMut) -> Result<usize, RuntimeError> {
//         match self.socket.send_to(buf, self.destination) {
//             Ok(num_bytes) => Ok(num_bytes),
//             Err(err) => Err(SendError(format!("Send error: {}", err))),
//         }
//     }
// }
//
// struct UdpReceiver {
//     socket: UdpSocket,
// }
//
// impl ReceiverEndPoint for UdpReceiver {
//     fn recv(&self, buf: &mut BytesMut) -> Result<usize, RuntimeError> {
//         match self.socket.recv_from(buf) {
//             Ok((num_bytes, _)) => Ok(num_bytes),
//             Err(err) => Err(ReceiveError(format!("Receive error: {}", err))),
//         }
//     }
// }