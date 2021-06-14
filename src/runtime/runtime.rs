use log::{error, info, debug};

use std::sync::mpsc::Sender as BlockingSender;

use std::net::{Ipv4Addr, SocketAddr};

use tokio::time::{Duration, interval};
use tokio::sync::mpsc::Receiver;
use tokio::net::UdpSocket;
use tokio::io::ErrorKind;

use crossterm::event::{KeyCode, KeyEvent};

use crate::events::Command;
use crate::model::config::ProtocolType::UDP;
use crate::model::config::{EndPoint, UdpCastType, Route, Config};
use crate::model::config::UdpCastType::{Unicast, Broadcast};

pub const DEFAULT_ROUTE_BUFFER_SIZE : usize = 1024;

pub async fn start_runtime_task(mut rt_rx: Receiver<Command>, to_gui_tx : BlockingSender<Command>) {
    // spawn gateway main task
    let _main_task = tokio::spawn(start_routes(to_gui_tx));

    while let Some(command) = rt_rx.recv().await {
        match command {
            Command::Quit => {
                break;
            }
            _ => {}
        }
    }
}

async fn start_routes(gui_tx : BlockingSender<Command>) {
    // let mut interval = interval(Duration::from_secs(2));
    // loop {
    //     interval.tick().await;
    //     gui_tx.send(Command::None).expect("Failure of sending Runtime Command to GUI task."); // no-op for now, to show a sign of life
    // }

    Config::current().routes.iter().for_each(|route| {
        tokio::spawn( start_route(route.clone()));
    });
}

async fn start_route(route : Route) {
    let src_addr : SocketAddr = format!("{}:{}", route.in_point.ip, route.in_point.port).parse().expect("Error parsing incoming Endpoint.");
    let dst_addr = format!("{}:{}", route.out_point.ip, route.out_point.port).parse().expect("Error parsing outgoing Endpoint.");

    debug!("Left point: {:?}", route.in_point);
    debug!("Right point: {:?}", route.out_point);

    // FIXME assuming as bidirectional udp to udp route, for testing
    let in_socket = create_udp_socket(&route.in_point).await;
    let out_socket = create_udp_socket(&route.out_point).await;

    let mut buf = vec![0u8; route.buffer_size];

    loop {
        let (len, addr) = in_socket.recv_from(&mut buf).await.expect("Error receiving from incoming Endpoint.");

        debug!("Received {} bytes from {}.", len, addr);

        match in_socket.try_send_to(&buf[..len], dst_addr) {
            Ok(n) => {
                break;
            }
            Err(ref e) if e.kind() == ErrorKind::WouldBlock => {
                continue;
            }
            Err(e) => {
                // return Err(e);
                error!("{}", e);
                break;
            }
        }
    }
}

async fn create_udp_socket(endpoint : &EndPoint) -> UdpSocket {
    let addr : SocketAddr = format!("{}:{}", endpoint.ip, endpoint.port).parse().expect("Error parsing Endpoint.");
    let socket = UdpSocket::bind(addr).await.expect("Error binding Endpoint.");
    socket.connect(addr).await.expect("Error connecting Endpoint.");
    if let UDP(cast_type) = &endpoint.protocol {
        if UdpCastType::Broadcast == cast_type.to_owned() {
            socket.set_broadcast(true).expect(format!("Failed to set socket to broadcast {:?}", socket).as_str());
        }
    }
    socket
}