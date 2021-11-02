use std::net::{IpAddr, Ipv4Addr, SocketAddrV4};
use std::fmt::{Display, Formatter};
use std::fmt;
use std::sync::Arc;

use log::{debug, error, trace};

use bytes::{Bytes, BytesMut};

use tokio::io::{AsyncReadExt, AsyncWriteExt, ErrorKind};
use tokio::net::{UdpSocket, TcpListener};
use tokio::net::tcp::{OwnedWriteHalf, OwnedReadHalf};
use tokio::sync::Semaphore;
use tokio::sync::broadcast::{channel as bc_channel, Receiver as BcReceiver, Sender as BcSender, Sender};
use tokio::sync::mpsc::{channel as mpsc_channel, Receiver as MpscReceiver, Sender as MpscSender};
use tokio::task::JoinHandle;
use tokio::time::Duration;

use crate::events::{Command, Event};
use crate::model::config::*;
use crate::model::config::FlowMode::BiDirectional;

const BROADCAST_TTL : u32 = 1; // only broadcast on the local subnet
const COMMAND_POLL_FREQ_MS : u64 = 200;

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


pub async fn start_runtime_task(mut command_rx: BcReceiver<Command>, event_tx: BcSender<Event>) {
    // spawn routes
    Config::current().routes.iter().for_each(|route| {
        let r = Arc::new(route.clone());
        let event_channel = event_tx.clone();
        tokio::spawn( async move {
            start_route(r, event_channel).await;
        });
    });

    // wait for runtime commands / shutdown.
    loop {
        tokio::time::sleep(Duration::from_millis(COMMAND_POLL_FREQ_MS)).await;
        let received_command = command_rx.try_recv();
        if let Ok(command) = received_command {
            match command {
                Command::Quit => {
                    break;
                }
                _ => {}
            }
        }
    }
}

async fn start_route(route : Arc<Route>, event_tx: BcSender<Event>) {
    // create separate endpoints, preferable generic
    // receive from the in_point, or both if bidirectional
    // allow for transformations after receiving
    // write to the out_point, or both if bidirectional

    match (&route.in_point.scheme, &route.out_point.scheme, &route.enabled) {
        (Scheme::UDP, Scheme::UDP, true) => { create_route_udp_udp(route, event_tx.clone()).await; }
        (Scheme::UDP, Scheme::TCP, true) => { create_route_udp_tcp(route, event_tx.clone()).await; }
        (Scheme::TCP, Scheme::TCP, true) => { create_route_tcp_tcp(route, event_tx.clone()).await; }
        (Scheme::TCP, Scheme::UDP, true) => { create_route_tcp_udp(route, event_tx.clone()).await; }
        _ => {}
    };
}

async fn create_route_udp_udp(route : Arc<Route>, event_tx: BcSender<Event>) {
    trace!("Creating route '{}'", route.name);

    let in_socket = Arc::new(create_udp_socket(&route.in_point).await);
    let out_socket = Arc::new(create_udp_socket(&route.out_point).await);

    let task_handle = {
        trace!("Spawn udp-udp route {} in-out", route.name);
        let out_address = format!("{}:{}",
                                  route.out_point.socket.ip(),
                                  route.out_point.socket.port());
        let mut buf = BytesMut::with_capacity(route.buffer_size);
        buf.resize(route.buffer_size, 0);

        tokio::spawn(
            run_udp_udp_route(route.clone(),
                              in_socket.clone(), out_socket.clone(), out_address,
                              buf, event_tx.clone())
        )
    };

    let task_handle_reverse = if BiDirectional == route.flow_mode {
        trace!("Spawn udp-udp route {} out-in", route.name);
        let in_address = format!("{}:{}",
                                  route.in_point.socket.ip(),
                                  route.in_point.socket.port());
        let mut buf = BytesMut::with_capacity(route.buffer_size);
        buf.resize(route.buffer_size, 0);

        Some(tokio::spawn(
            run_udp_udp_route(route.clone(),
                              out_socket.clone(), in_socket.clone(), in_address,
                              buf, event_tx.clone())
        ))
    } else { None };

    // TODO use tokio::select! on all 2 handles?
    task_handle.await.expect(format!("Route {} task (udp-udp > in-out) panicked", route.name).as_str());
    if let Some(handle) = task_handle_reverse {
        handle.await.expect(format!("Route {} task (udp-udp > out-in) panicked", route.name).as_str());
    }
}

/// A route that connects a UDP socket to a TCP Server socket.
/// Bytes received via the UDP socket are forwarded to connections attached to the TCP socket.
async fn create_route_udp_tcp(route : Arc<Route>, event_tx: BcSender<Event>) {
    trace!("Creating udp-tcp route '{}'", route.name);

    let in_socket = Arc::new(create_udp_socket(&route.in_point).await);
    let out_socket = create_tcp_server_socket(&route.out_point).await;

    // TODO move inside run_ functions?
    let mut buf = BytesMut::with_capacity(route.buffer_size);
    buf.resize(route.buffer_size, 0);

    // TODO make channel capacities a setting
    let (to_tcp_sender, _to_tcp_receiver) : (BcSender<Bytes>, BcReceiver<Bytes>) = bc_channel(10);
    let (from_tcp_sender, from_tcp_receiver) : (MpscSender<Bytes>, MpscReceiver<Bytes>) = mpsc_channel(10);

    // accept loop for incoming TCP connections
    let _handle_accept = {
        let from_tcp_sender_opt = if BiDirectional == route.flow_mode {
            Some(from_tcp_sender.clone())
        } else { None };
        tokio::spawn(
            run_tcp_accept_loop(route.clone(), out_socket, Some(to_tcp_sender.clone()), from_tcp_sender_opt, event_tx.clone())
        )
    };

    let task_handle = tokio::spawn(
        run_udp_tcp_server_route(route.clone(), in_socket.clone(), to_tcp_sender.clone(), buf, event_tx.clone())
    );

    let task_handle_reverse = if BiDirectional == route.flow_mode {
        let out_address = format!("{}:{}",
                                  route.in_point.socket.ip(),
                                  route.in_point.socket.port());
        Some(tokio::spawn(
            run_tcp_server_udp_route(route.clone(), from_tcp_receiver, in_socket.clone(), out_address, event_tx.clone())
        ))
    } else { None };

    // TODO use tokio::select! on all 3 handles?
    task_handle.await.expect(format!("Route {} task (udp->tcp) panicked", route.name).as_str());
    if let Some(handle) = task_handle_reverse {
        handle.await.expect(format!("Route {} task (udp<-tcp) panicked", route.name).as_str());
    }
}

async fn create_route_tcp_tcp(route : Arc<Route>, event_tx: BcSender<Event>) {
    let in_socket = create_tcp_server_socket(&route.in_point).await;
    let out_socket = create_tcp_server_socket(&route.out_point).await;

    let (from_tcp_sender, from_tcp_receiver) : (MpscSender<Bytes>, MpscReceiver<Bytes>) = mpsc_channel(10);
    let (from_tcp_sender_reverse, from_tcp_receiver_reverse) : (MpscSender<Bytes>, MpscReceiver<Bytes>) = mpsc_channel(10);
    let (to_tcp_sender, _to_tcp_receiver) : (BcSender<Bytes>, BcReceiver<Bytes>) = bc_channel(10);
    let (to_tcp_sender_reverse, _to_tcp_receiver_reverse) : (BcSender<Bytes>, BcReceiver<Bytes>) = bc_channel(10);

    let _handle_accept_in_socket = {
        let to_tcp_sender_reverse_opt = if BiDirectional == route.flow_mode {
            Some(to_tcp_sender_reverse.clone())
        } else { None };
        tokio::spawn(
            run_tcp_accept_loop(route.clone(), in_socket, to_tcp_sender_reverse_opt, Some(from_tcp_sender.clone()), event_tx.clone())
        )
    };
    let _handle_accept_out_socket = {
        let from_tcp_sender_reverse_opt = if BiDirectional == route.flow_mode {
            Some(from_tcp_sender_reverse.clone())
        } else { None };
        tokio::spawn(
            run_tcp_accept_loop(route.clone(), out_socket, Some(to_tcp_sender.clone()), from_tcp_sender_reverse_opt, event_tx.clone())
        )
    };

    let task_handle = tokio::spawn(
        run_tcp_server_tcp_server_route(route.clone(), from_tcp_receiver, to_tcp_sender.clone(), event_tx.clone())
    );

    let task_handle_reverse = if BiDirectional == route.flow_mode {
        Some(tokio::spawn(
            run_tcp_server_tcp_server_route(route.clone(), from_tcp_receiver_reverse, to_tcp_sender_reverse.clone(), event_tx.clone())
        ))
    } else { None };

    // TODO use tokio::select! on all 4 handles?
    task_handle.await.expect(format!("Route {} task (tcp->tcp) panicked", route.name).as_str());
    if let Some(handle) = task_handle_reverse {
        handle.await.expect(format!("Route {} task (tcp<-tcp) panicked", route.name).as_str());
    }
}

async fn create_route_tcp_udp(route : Arc<Route>, event_tx: BcSender<Event>) {
    trace!("Creating tcp-udp route '{}'", route.name);

    let in_socket = create_tcp_server_socket(&route.in_point).await;
    let out_socket = Arc::new(create_udp_socket(&route.out_point).await);

    // TODO move inside run_ functions?
    let mut buf = BytesMut::with_capacity(route.buffer_size);
    buf.resize(route.buffer_size, 0);

    // TODO make channel capacities a setting
    let (from_tcp_sender, from_tcp_receiver) : (MpscSender<Bytes>, MpscReceiver<Bytes>) = mpsc_channel(10);
    let (to_tcp_sender, _to_tcp_receiver) : (BcSender<Bytes>, BcReceiver<Bytes>) = bc_channel(10);

    // accept loop for incoming TCP connections
    let _handle_accept = {
        let to_tcp_sender_opt = if BiDirectional == route.flow_mode {
            Some(to_tcp_sender.clone())
        } else { None };
        tokio::spawn(
            run_tcp_accept_loop(route.clone(), in_socket, to_tcp_sender_opt, Some(from_tcp_sender), event_tx.clone())
        )
    };

    let task_handle = {
        let out_address = format!("{}:{}",
                                  route.out_point.socket.ip(),
                                  route.out_point.socket.port());
        tokio::spawn(
            run_tcp_server_udp_route(route.clone(), from_tcp_receiver, out_socket.clone(), out_address, event_tx.clone())
        )
    };

    let task_handle_reverse = if BiDirectional == route.flow_mode {
        Some(tokio::spawn(
            run_udp_tcp_server_route(route.clone(), out_socket.clone(), to_tcp_sender.clone(), buf, event_tx.clone())
        ))
    } else { None };

    // TODO use tokio::select! on all 3 handles?
    task_handle.await.expect(format!("Route {} task (udp->tcp) panicked", route.name).as_str());
    if let Some(handle) = task_handle_reverse {
        handle.await.expect(format!("Route {} task (udp<-tcp) panicked", route.name).as_str());
    }
}

async fn create_udp_socket(endpoint : &EndPoint) -> UdpSocket {
    // TODO try to bind to a specific interface/address
    // let local_addr = SocketAddrV4::new(*endpoint.socket.ip(), endpoint.socket.port());
    let local_addr = SocketAddrV4::new(Ipv4Addr::UNSPECIFIED, endpoint.socket.port());
    let socket = UdpSocket::bind(local_addr)
        .await
        .expect(format!("Error binding Endpoint to local port {}.", local_addr).as_str());
    trace!("UDP socket created: {:?} - local addr: {:?}", socket, socket.local_addr());

    // set type options
    match endpoint.socket_type {
        SocketType::UdpSocketType(UdpSocketType::Unicast) => {
            socket.set_ttl(endpoint.ttl)
                .expect(format!("Error setting TTL for unicast UDP socket {:?}.", endpoint.socket).as_str());
        },
        SocketType::UdpSocketType(UdpSocketType::Broadcast) => {
            socket.set_broadcast(true)
                .expect(format!("Error setting broadcast for UDP socket {:?}.", endpoint.socket).as_str());
            socket.set_ttl(BROADCAST_TTL)
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

async fn create_tcp_server_socket(endpoint : &EndPoint) -> TcpListener {
    let listener_addr = SocketAddrV4::new(Ipv4Addr::UNSPECIFIED, endpoint.socket.port());
    let listener = TcpListener::bind(listener_addr)
        .await
        .expect(format!("Error binding TCP listener socket for {:?}", listener_addr).as_str());
    trace!("TCP listener socket created: {:?}", listener);

    listener
}

async fn run_tcp_accept_loop(route : Arc<Route>, socket : TcpListener, to_tcp_sender_opt: Option<BcSender<Bytes>>, from_tcp_receiver_opt: Option<MpscSender<Bytes>>, event_tx: BcSender<Event>) {
    trace!("Waiting for incoming TCP connections on {:?}", socket.local_addr());

    let sem = Arc::new(Semaphore::new(route.max_connections));

    loop {
        let (stream, _addr) = socket.accept().await
            .expect(format!("Error establishing incoming TCP connection for route '{}'.", route.name).as_str());
        if let Ok(_guard) = sem.try_acquire() {
            let (reader, writer) = stream.into_split();

            // TODO clean up this part if we don't need the task handles
            let (write_handle_opt, read_handle_opt) =
            match (&to_tcp_sender_opt, &from_tcp_receiver_opt) {
                (Some(ref to_tcp_sender), Some(ref from_tcp_receiver)) => {
                    (Some(tokio::spawn(
                        run_tcp_server_writer(route.clone(),
                                              writer,
                                              to_tcp_sender.subscribe(),
                                              event_tx.clone())
                    )),
                     Some(tokio::spawn(
                        run_tcp_server_reader(route.clone(),
                                              reader,
                                              None,
                                              from_tcp_receiver.clone(),
                                              event_tx.clone())
                    )))
                }
                (Some(ref to_tcp_sender), None) => {
                    (Some(tokio::spawn(
                        run_tcp_server_writer(route.clone(),
                                              writer,
                                              to_tcp_sender.subscribe(),
                                              event_tx.clone())
                    )),None)
                }
                (None, Some(ref from_tcp_receiver)) => {
                    (None,
                     Some(tokio::spawn(
                        run_tcp_server_reader(route.clone(),
                                              reader,
                                              Some(writer),
                                              from_tcp_receiver.clone(),
                                              event_tx.clone())
                    )))
                }
                (None, None) => {break;}
            };
        } else { trace!("Rejecting connection: too many open sockets on {:?}", socket.local_addr()); }
    }
}

async fn run_tcp_server_writer(route : Arc<Route>, mut writer: OwnedWriteHalf, mut data_channel : BcReceiver<Bytes>, event_tx: BcSender<Event>) {
    loop {
        // TODO how does this handle late joiners? - seems OK: late joiners only get future messages
        if let Ok(out_buf) = data_channel.recv().await {
            trace!("Got via BcReceiver channel - will write via TCP: {:?}", &out_buf[..]);
            match writer.write(&out_buf[..]).await {
                Ok(0) => { break; }
                Ok(bytes_send) => {
                    if let Err(msg) = event_tx.send(Event::StatBytesSend(0, bytes_send)) {
                        trace!("Error sending runtime send statistics for route '{}' through channel: {}", route.name, msg);
                    } else {
                        trace!("StatBytesSend send through channel for TCP socket '{:?}'", writer);
                    }
                }
                Err(err) => {
                    trace!("Error sending data through TCP socket '{:?}': {:?}", writer, err);
                    break;
                }
            }
        }
    }
}

async fn run_tcp_server_reader(route : Arc<Route>, mut reader: OwnedReadHalf, writer: Option<OwnedWriteHalf>, data_channel : MpscSender<Bytes>, event_tx: BcSender<Event>) {
    let mut buf = BytesMut::with_capacity(route.buffer_size);
    buf.resize(route.buffer_size, 0);
    loop {
        trace!("Waiting for incoming TCP data");
        match reader.read(&mut buf).await {
            Ok(0) => {
                // reunite not really needed, but need to prevent the writer from being dropped
                //  when the endpoint is unidirectional
                if let Some(w) = writer { reader.reunite(w); }
                break; } // Indicates the connection is closed.
            Ok(bytes_received) => {
                trace!("received bytes via tcp: {} bytes - {:?}", bytes_received, &buf[..bytes_received]);
                // TODO StatsBytesReceived
                let buf_to_send = Bytes::copy_from_slice(&buf[..bytes_received]);
                trace!("buf_to_send: {:?}", buf_to_send);
                event_tx.send(Event::StatBytesReceived(0, bytes_received)).expect("Error sending StatBytesReceived for incoming TCP socket.");
                data_channel.send(buf_to_send).await.expect("Error sending received data through mpsc channel.");
                trace!("Did send {} bytes buf_to_send via MpscChannel", bytes_received);
            }
            Err(err) => {
                trace!("Error while receiving data via tcp: {}", err);
                break;
            }
        }
    }
}

fn message_is_from_own_host(host: IpAddr, received_from : IpAddr) -> bool {
    return host == received_from;
}

async fn run_udp_udp_route(route : Arc<Route>,
                           in_socket : Arc<UdpSocket>, out_socket : Arc<UdpSocket>, out_address : String,
                           mut buf: BytesMut,
                           route_data_tx : BcSender<Event>) {
    loop {
        let (bytes_received, from_address) = in_socket.recv_from(&mut buf).await.expect("Error receiving from incoming Endpoint.");

        if route.block_host &&
            message_is_from_own_host(route.in_point.interface, from_address.ip()) { continue }

        // collect statistics for rx
        if let Err(msg) = route_data_tx.send(Event::StatBytesReceived(0, bytes_received)) {
            trace!("Error sending runtime receive statistics for route '{}' through channel: {}", route.name, msg);
        } else {
            trace!("StatBytesReceived send through channel for route '{}'", route.name);
        }

        trace!("Received {} bytes from {}.", bytes_received, from_address);
        // TODO filter/transform

        match out_socket.send_to(&buf[..bytes_received],
                                 out_address.as_str()).await {
            Ok(bytes_send) => {
                trace!("Successfully send {:?} bytes to {:?} through route '{}'", bytes_send, out_address.as_str(), route.name);
                // collect statistics for tx
                if let Err(msg) = route_data_tx.send(Event::StatBytesSend(0, bytes_send)) {
                    trace!("Error sending runtime send statistics for route '{}' through channel: {}", route.name, msg);
                } else {
                    trace!("StatBytesSend send through channel for route '{}'", route.name);
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

async fn run_udp_tcp_server_route(route : Arc<Route>,
                                  in_socket : Arc<UdpSocket>, out_sender : BcSender<Bytes>,
                                  mut buf: BytesMut,
                                  event_tx: BcSender<Event>) {
    loop {
        let (bytes_received, from_address) = in_socket.recv_from(&mut buf).await.expect("Error receiving from incoming Endpoint.");

        if route.block_host &&
            message_is_from_own_host(route.in_point.interface, from_address.ip()) { continue }

        // collect statistics for rx
        if let Err(msg) = event_tx.send(Event::StatBytesReceived(0, bytes_received)) {
            trace!("Error sending runtime receive statistics for route '{}' through channel: {}", route.name, msg);
        } else {
            trace!("StatBytesReceived send through channel");
        }

        // TODO filter/transform
        let send_buf = Bytes::copy_from_slice(&buf[..bytes_received]);
        let bytes_produced = send_buf.len();

        let _bytes_out = out_sender.send(send_buf.clone()).expect("Error forwarding outgoing data to sending sockets");
        // FIXME collect outgoing statistics here, or in TCP writer?
        if let Err(msg) = event_tx.send(Event::StatBytesSend(0, bytes_produced)) {
            trace!("Error sending runtime receive statistics for route '{}' through channel: {}", route.name, msg);
        } else {
            trace!("StatBytesReceived send through channel");
        }
    }
}

async fn run_tcp_server_udp_route(route : Arc<Route>, mut in_receiver: MpscReceiver<Bytes>, out_socket : Arc<UdpSocket>, out_address : String, event_tx: BcSender<Event>) {
    loop {
        let bytes = in_receiver.recv().await.expect("Error receiving from incoming endpoint channel.");
        trace!("got from from_tcp MpscReceiver channel: {:?}", bytes);
        // collect statistics for rx
        if let Err(msg) = event_tx.send(Event::StatBytesReceived(0, bytes.len())) {
            trace!("Error sending runtime receive statistics for route '{}' through channel: {}", route.name, msg);
        } else {
            trace!("StatBytesReceived send through channel");
        }

        // TODO filter/transform

        match out_socket.send_to(&bytes[..], out_address.as_str()).await {
            Ok(bytes_send) => {
                trace!("wrote {} bytes via UDP", bytes_send);
                // collect statistics for tx
                if let Err(msg) = event_tx.send(Event::StatBytesSend(0, bytes_send)) {
                    trace!("Error sending runtime send statistics for route '{}' through channel: {}", route.name, msg);
                } else {
                    trace!("StatBytesSend send through channel for route '{}'", route.name);
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
    }
}

// FIXME route sends received messages out via the same socket instead of the other
async fn run_tcp_server_tcp_server_route(route : Arc<Route>, mut in_receiver: MpscReceiver<Bytes>, out_sender : BcSender<Bytes>, event_tx: BcSender<Event>) {
    loop {
        let bytes = in_receiver.recv().await.expect("Error receiving from incoming endpoint channel.");
        trace!("got from from_tcp MpscReceiver channel: {:?}", bytes);

        // collect statistics for rx
        if let Err(msg) = event_tx.send(Event::StatBytesReceived(0, bytes.len())) {
            trace!("Error sending runtime receive statistics for route '{}' through channel: {}", route.name, msg);
        } else {
            trace!("StatBytesReceived send through channel");
        }

        // TODO filter/transform

        // let send_buf = Bytes::copy_from_slice(&buf[..bytes_received]);
        // let bytes_produced = send_buf.len();

        let bytes_produced = bytes.len();
        let _bytes_out = out_sender.send(bytes.clone()).expect("Error forwarding outgoing data to sending sockets");
        // FIXME collect outgoing statistics here, or in TCP writer?
        if let Err(msg) = event_tx.send(Event::StatBytesSend(0, bytes_produced)) {
            trace!("Error sending runtime receive statistics for route '{}' through channel: {}", route.name, msg);
        } else {
            trace!("StatBytesReceived send through channel");
        }
    }
}