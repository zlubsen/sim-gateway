use log::{error, info, debug, trace};

use std::net::{Ipv4Addr, SocketAddr};

use tokio::time::{Duration, interval};
use tokio::net::UdpSocket;
use tokio::io::ErrorKind;

use tokio::sync::broadcast::{Receiver, Sender};

use crossterm::event::{KeyCode, KeyEvent};

use crate::events::{Command, Event};
use crate::model::config::Scheme::UDP;
use crate::model::config::{EndPoint, UdpCastType, Route, Config};
use crate::model::config::UdpCastType::{Unicast, Broadcast};

pub const DEFAULT_ROUTE_BUFFER_SIZE : usize = 1024;

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
    let src_addr : SocketAddr = route.in_point.socket;
    let dst_addr = route.out_point.socket;

    debug!("Left point: {:?}", route.in_point);
    debug!("Right point: {:?}", route.out_point);

    // FIXME assuming as bidirectional udp to udp route, for testing
    let in_socket = create_udp_socket(&route.in_point).await;
    let out_socket = create_udp_socket(&route.out_point).await;

    let mut buf = vec![0u8; route.buffer_size];

    loop {
        let (len, addr) = in_socket.recv_from(&mut buf).await.expect("Error receiving from incoming Endpoint.");

        debug!("Received {} bytes from {}.", len, addr);

        match out_socket.try_send_to(&buf[..len], dst_addr) {
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
    let addr : SocketAddr = endpoint.socket;
    let socket = UdpSocket::bind(addr).await.expect("Error binding Endpoint.");
    socket.connect(addr).await.expect("Error connecting Endpoint.");
    // if let UDP(cast_type) = &endpoint.protocol {
    //     if UdpCastType::Broadcast == cast_type.to_owned() {
    //         socket.set_broadcast(true).expect(format!("Failed to set socket to broadcast {:?}", socket).as_str());
    //     }
    // }
    socket
}