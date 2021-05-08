use std::sync::mpsc::Sender as BlockingSender;

use std::net::{Ipv4Addr, SocketAddr};

use tokio::time::{Duration, interval};
use tokio::sync::mpsc::Receiver;
use tokio::net::UdpSocket;
use tokio::io::ErrorKind;

use crossterm::event::{KeyCode, KeyEvent};

use crate::events::Command;
use crate::runtime::ProtocolType::UDP;
use crate::runtime::UdpCastType::{Broadcast, Unicast};

pub const DEFAULT_ROUTE_BUFFER_SIZE : usize = 1024;

#[derive(Clone)]
pub struct Route {
    in_point : EndPoint,
    out_point : EndPoint,
    buffer_size : usize,
    flow_mode : FlowMode,
    // filters_in : Vec<Filter>,
    // filters_out : Vec<Filter>,
    // transforms : Vec<Transform>
}

impl Route {
    pub fn new(in_point : EndPoint, out_point : EndPoint, buffer_size : usize, flow_mode : FlowMode) -> Route {
        return Route {
            in_point,
            out_point,
            buffer_size,
            flow_mode,
        }
    }
}

#[derive(Debug)]
pub struct EndPoint {
    ip : Ipv4Addr,
    port : u16,
    protocol : ProtocolType,
}

#[derive(Debug)]
pub enum ProtocolType {
    UDP(UdpCastType),
    TCP,
}

#[derive(Debug)]
pub enum UdpCastType {
    Unicast,
    Broadcast,
    Multicast,
}

#[derive(Debug)]
pub enum FlowMode {
    UniDirectional,
    BiDirectional,
}

// trait Filter {
//     fn new() -> Self;
//     fn apply(&self) -> bool;
// }
//
// trait Transform {
//     fn new() -> Self;
//     fn apply(&self) -> &self;
// }

pub async fn start_runtime_task(mut rt_rx: Receiver<Command>, to_gui_tx : BlockingSender<Command>) {
    let in_point = EndPoint {
        ip : Ipv4Addr::UNSPECIFIED,
        port : 3010,
        protocol : UDP(Unicast),
    };
    let end_point = EndPoint {
        ip : Ipv4Addr::new(192,168,8,255),
        port : 3000,
        protocol : UDP(Broadcast),
    };

    // spawn gateway main task
    let _main_task = tokio::spawn(start_routes(to_gui_tx, in_point, end_point) );

    while let Some(command) = rt_rx.recv().await {
        match command {
            Command::Quit => {
                break;
            }
            _ => {}
        }
    }
}

async fn start_routes(gui_tx : BlockingSender<Command>, routes : &Vec<Route>) {
    // let mut interval = interval(Duration::from_secs(2));
    // loop {
    //     interval.tick().await;
    //     gui_tx.send(Command::None).expect("Failure of sending Runtime Command to GUI task."); // no-op for now, to show a sign of life
    // }

    routes.iter().for_each(|route| {
        tokio::spawn( start_route(route.clone()));
    });
}

async fn start_route(route : Route) {
    let src_addr : SocketAddr = format!("{}:{}", route.in_point.ip, route.in_point.port).parse().expect("Error parsing incoming Endpoint.");
    let dst_addr = format!("{}:{}", route.out_point.ip, route.out_point.port).parse().expect("Error parsing outgoing Endpoint.");

    println!("Left point: {:?}", route.in_point);
    println!("Right point: {:?}", route.out_point);

    // FIXME assuming as bidirectional udp to udp route, for testing
    let in_socket = create_udp_socket(route.in_point);
    let out_socket = create_udp_socket(route.out_point);

    let mut buf = vec![0u8; route.buffer_size];

    loop {
        let (len, addr) = in_socket.recv_from(&mut buf).await.expect("Error receiving from incoming Endpoint.");

        println!("Received {} bytes from {}.", len, addr);

        match in_socket.try_send_to(&buf[..len], dst_addr) {
            Ok(n) => {
                break;
            }
            Err(ref e) if e.kind() == ErrorKind::WouldBlock => {
                continue;
            }
            Err(e) => {
                // return Err(e);
                println!("{}", e);
                break;
            }
        }
    }
}

fn create_udp_socket(endpoint : EndPoint) -> UdpSocket {
    let addr : SocketAddr = format!("{}:{}", endpoint.ip, endpoint.port).parse().expect("Error parsing Endpoint.");
    let socket = UdpSocket::bind(addr).await.expect("Error binding Endpoint.");
    socket.connect(addr).await.expect("Error connecting Endpoint.");
    if let UDP(cast_type) = endpoint.protocol {
        if UdpCastType::Broadcast == cast_type {
            socket.set_broadcast(true);
        }
    }
    socket
}