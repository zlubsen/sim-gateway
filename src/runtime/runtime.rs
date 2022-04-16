use std::net::{IpAddr, Ipv4Addr, SocketAddr, SocketAddrV4};
use std::fmt::{Display, Formatter};
use std::fmt;
use std::sync::Arc;

use log::{info, error, trace};

use bytes::{Bytes, BytesMut};

use tokio::io::{AsyncReadExt, AsyncWriteExt, ErrorKind};
use tokio::net::{UdpSocket, TcpListener, TcpStream};
use tokio::net::tcp::{OwnedWriteHalf, OwnedReadHalf};
use tokio::sync::Semaphore;
use tokio::sync::broadcast::{channel as bc_channel, Receiver as BcReceiver, Sender as BcSender};
use tokio::sync::mpsc::{channel as mpsc_channel, Receiver as MpscReceiver, Sender as MpscSender};
use tokio::sync::Notify;
use tokio::task::JoinHandle;
use tokio::time::{Duration, interval};

use crate::events::{Command, Event, Statistics};
use crate::model::config::*;
use crate::model::config::FlowMode::BiDirectional;
use crate::model::constants::*;
use crate::model::filters::passes_filters;
use crate::model::transformers::apply_transformers;

type TcpIn = (BytesMut, SocketAddr);
type TcpHubOut = (Bytes, SocketAddr);

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


pub async fn start_runtime(mut command_rx: BcReceiver<Command>, event_tx: BcSender<Event>) {
    // spawn routes
    let route_handles : Vec<JoinHandle<()>> = Config::current().routes.iter().filter(|r|r.enabled).map(|route| {
        let r = Arc::new(route.clone());
        let event_channel = event_tx.clone();
        tokio::spawn( async move {
            start_route(r, event_channel).await;
        })
    }).collect();

    // spawn hubs
    let hub_handles : Vec<JoinHandle<()>> = Config::current().hubs.iter().filter(|h|h.enabled).map(|hub| {
        let h = Arc::new(hub.clone());
        let event_channel = event_tx.clone();
        trace!("spawn start_hub");
        tokio::spawn( async move {
            start_hub(h, event_channel).await;
        })
    }).collect();

    // TODO review channel capacity
    let (stats_tx , stats_rx) : (MpscSender<Statistics>, MpscReceiver<Statistics>) = mpsc_channel(STATS_CHANNEL_CAPACITY);
    let stats_collector_handle =
        tokio::spawn(run_stats_collector(event_tx.subscribe(), stats_tx));

    // listen for events when running headless
    let cli_msg_handle = if Config::current().mode == Mode::Headless {
        Some(tokio::spawn(run_cli_messenger(event_tx.subscribe())))
    } else { None };

    // print statistics when running headless
    let cli_stats_printer_handle = if Config::current().mode == Mode::Headless {
        Some(tokio::spawn(run_stats_cli_printer(stats_rx)))
    } else { None };

    // wait for runtime commands / shutdown.
    loop {
        if let Ok(command) = command_rx.recv().await {
            match command {
                Command::Quit => {
                    // cleanup tasks
                    if let Some(handle) = cli_msg_handle { handle.abort(); }
                    if let Some(handle) = cli_stats_printer_handle { handle.abort(); }
                    stats_collector_handle.abort();
                    for r in route_handles {
                        r.abort();
                    }
                    for h in hub_handles {
                        h.abort();
                    }
                    break;
                }
                _ => {}
            }
        }
    }
}

/// Spawns a task that listens for gateway events and prints
/// selected types (Event::Message, Event::Error) to the stdout (CLI).
/// Prints at info or error log levels.
async fn run_cli_messenger(mut event_rx: BcReceiver<Event>) {
    loop {
        if let Ok(event) = event_rx.recv().await {
            match event {
                Event::Message(msg) => { info!("{}", msg); }
                Event::Error(msg) => { error!("{}", msg); }
                _ => {}
            }
        }
    }
}

/// Receives, processes and periodically publishes statistics
/// on the data flowing through the gate
async fn run_stats_collector(mut event_rx: BcReceiver<Event>, publisher: MpscSender<Statistics>) {
    let mut total_rx : u64 = 0;
    let mut total_tx: u64 = 0;
    let mut window_rx: f64 = 0.0;
    let mut window_tx: f64 = 0.0;

    let mut tcp_connections : usize = 0;

    let mut interval = interval(Duration::from_secs(STATS_AGGREGATION_WINDOW_SECS));

    loop {
        tokio::select! {
            _ = interval.tick() => {
                let avg_rx = window_rx / STATS_AGGREGATION_WINDOW_SECS as f64;
                let avg_tx = window_tx / STATS_AGGREGATION_WINDOW_SECS as f64;
                publisher.send(Statistics::AverageBytes(0, avg_rx, avg_tx)).await.expect("Error publishing stats");
                window_rx = 0.0;
                window_tx = 0.0;
            }
            result = event_rx.recv() => {
                if let Ok(event) = result {
                    match event {
                        Event::Message(_) => {}
                        Event::Error(_) => {}
                        Event::SocketConnected(_scheme, _address) => {
                            tcp_connections += 1;
                            // TODO handle without trace!()
                            trace!("TCP socket connected: {} - total connections: {}", _address, tcp_connections);
                        }
                        Event::SocketDisconnected(_scheme, _address) => {
                            // TODO handle without trace!()
                            tcp_connections -= 1;
                            trace!("TCP socket disconnected: {} - total connections: {}", _address, tcp_connections);
                        }
                        Event::StatBytesReceived(_id, num_bytes) => {
                            total_rx += num_bytes as u64;
                            window_rx += num_bytes as f64;
                            publisher.send(Statistics::TotalBytes(0, total_rx, total_tx)).await.expect("Error publishing stats");
                        }
                        Event::StatBytesSend(_id, num_bytes) => {
                            total_tx += num_bytes as u64;
                            window_tx += num_bytes as f64;
                            publisher.send(Statistics::TotalBytes(0, total_rx, total_tx)).await.expect("Error publishing stats");
                        }
                        Event::ErrorTooManyConnections(_, _) => {}
                    }
                }
            }
        }
    }
}

async fn run_stats_cli_printer(mut stats_rx: MpscReceiver<Statistics>) {
    let mut total_rx : u64 = 0;
    let mut total_tx : u64 = 0;
    let mut avg_rx : f64 = 0.0;
    let mut avg_tx : f64 = 0.0;

    let mut interval = interval(STATS_PRINTER_RATE_MS);
    loop {
        tokio::select! {
            _ = interval.tick() => {
                info!("Average bytes: {} rx / {} tx", avg_rx, avg_tx);
                info!("Total bytes: {} rx / {} tx", total_rx, total_tx);
            }
            stats = stats_rx.recv() => {
                if let Some(event) = stats {
                    match event {
                        Statistics::TotalBytes(_id, rx, tx) => {
                            total_rx = rx;
                            total_tx = tx;
                        }
                        Statistics::AverageBytes(_id, rx, tx) => {
                            avg_rx = rx;
                            avg_tx = tx;
                        }
                    }
                }
            }
        };
    }
}

async fn start_route(route : Arc<Route>, event_tx: BcSender<Event>) {
    match (&route.in_point.scheme, &route.in_point.socket_type, &route.out_point.scheme, &route.out_point.socket_type) {
        (Scheme::UDP, _, Scheme::UDP, _)
            => { create_route_udp_udp(route, event_tx).await; }
        (Scheme::UDP, _, Scheme::TCP, SocketType::TcpSocketType(TcpSocketType::Server))
            => { create_route_udp_tcp_server(route, event_tx).await; }
        (Scheme::UDP, _, Scheme::TCP, SocketType::TcpSocketType(TcpSocketType::Client))
        => { create_route_udp_tcp_client(route, event_tx).await; }
        (Scheme::TCP, SocketType::TcpSocketType(TcpSocketType::Server), Scheme::TCP, SocketType::TcpSocketType(TcpSocketType::Server))
            => { create_route_tcp_tcp_servers(route, event_tx).await; }
        (Scheme::TCP, SocketType::TcpSocketType(TcpSocketType::Client), Scheme::TCP, SocketType::TcpSocketType(TcpSocketType::Server))
            => { create_route_tcp_client_tcp_server(route, event_tx).await; }
        (Scheme::TCP, SocketType::TcpSocketType(TcpSocketType::Server), Scheme::TCP, SocketType::TcpSocketType(TcpSocketType::Client))
            => { create_route_tcp_server_tcp_client(route, event_tx).await; }
        (Scheme::TCP, SocketType::TcpSocketType(TcpSocketType::Client), Scheme::TCP, SocketType::TcpSocketType(TcpSocketType::Client))
            => { create_route_tcp_tcp_clients(route, event_tx).await }
        (Scheme::TCP, SocketType::TcpSocketType(TcpSocketType::Server), Scheme::UDP, _)
            => { create_route_tcp_server_udp(route, event_tx).await; }
        (Scheme::TCP, SocketType::TcpSocketType(TcpSocketType::Client), Scheme::UDP, _)
            => { create_route_tcp_client_udp(route, event_tx).await; }
        (_,_,_,_) => { event_tx.send(Event::Error(format!("Invalid route definition for '{}'", route.name))).unwrap_or_default(); }
    };
}

async fn create_route_udp_udp(route : Arc<Route>, event_tx: BcSender<Event>) {
    event_tx.send(Event::Message(format!("Creating udp-udp route '{}'", route.name))).unwrap_or_default();

    let in_socket = Arc::new(create_udp_socket(&route.in_point).await);
    let out_socket = Arc::new(create_udp_socket(&route.out_point).await);

    let task_handle = {
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

    task_handle.await.expect(format!("Route {} task (udp-udp > in-out) panicked", route.name).as_str());
    if let Some(handle) = task_handle_reverse {
        handle.await.expect(format!("Route {} task (udp-udp > out-in) panicked", route.name).as_str());
    }
}

/// A route that connects a UDP socket to a TCP Server socket.
/// Bytes received via the UDP socket are forwarded to connections attached to the TCP socket.
async fn create_route_udp_tcp_server(route : Arc<Route>, event_tx: BcSender<Event>) {
    event_tx.send(Event::Message(format!("Creating udp-tcp(server) route '{}'", route.name))).unwrap_or_default();

    let in_socket = Arc::new(create_udp_socket(&route.in_point).await);
    let out_socket = create_tcp_server_socket(&route.out_point).await;

    // TODO move inside run_ functions?
    let mut buf = BytesMut::with_capacity(route.buffer_size);
    buf.resize(route.buffer_size, 0);

    let (to_tcp_sender, _to_tcp_receiver) : (BcSender<Bytes>, BcReceiver<Bytes>) = bc_channel(SOCKET_CHANNEL_CAPACITY);
    let (from_tcp_sender, from_tcp_receiver) : (MpscSender<TcpIn>, MpscReceiver<TcpIn>) = mpsc_channel(SOCKET_CHANNEL_CAPACITY);

    // accept loop for incoming TCP connections
    let handle_accept = {
        let from_tcp_sender_opt = if BiDirectional == route.flow_mode {
            Some(from_tcp_sender.clone())
        } else { None };
        tokio::spawn(
            run_tcp_accept_loop(route.clone(), out_socket,
                                Some(to_tcp_sender.clone()), from_tcp_sender_opt,
                                event_tx.clone())
        )
    };

    let task_handle = tokio::spawn(
        run_udp_tcp_route(route.clone(), in_socket.clone(),
                          to_tcp_sender.clone(), buf, event_tx.clone())
    );
    let task_handle_reverse = if BiDirectional == route.flow_mode {
        let out_address = format!("{}:{}",
                                  route.in_point.socket.ip(),
                                  route.in_point.socket.port());
        Some(tokio::spawn(
            run_tcp_udp_route(route.clone(), from_tcp_receiver,
                              in_socket.clone(), out_address, event_tx.clone())
        ))
    } else { None };

    task_handle.await.expect(format!("Route {} task udp->tcp(server) panicked", route.name).as_str());
    if let Some(handle) = task_handle_reverse {
        handle.await.expect(format!("Route {} task udp<-tcp(server) panicked", route.name).as_str());
    }
    handle_accept.await.expect(format!("Route {} task udp->tcp(server) panicked - accept loop", route.name).as_str());
}

async fn create_route_udp_tcp_client(route : Arc<Route>, event_tx: BcSender<Event>) {
    event_tx.send(Event::Message(format!("Creating udp-tcp(client) route '{}'", route.name))).unwrap_or_default();

    let in_socket = Arc::new(create_udp_socket(&route.in_point).await);
    let (out_socket_reader, out_socket_writer) = create_tcp_client_socket(&route.out_point).await.into_split();

    // TODO move inside run_ functions?
    let mut buf = BytesMut::with_capacity(route.buffer_size);
    buf.resize(route.buffer_size, 0);

    let (to_tcp_sender, to_tcp_receiver) : (BcSender<Bytes>, BcReceiver<Bytes>) = bc_channel(SOCKET_CHANNEL_CAPACITY);
    let (from_tcp_sender, from_tcp_receiver) : (MpscSender<TcpIn>, MpscReceiver<TcpIn>) = mpsc_channel(SOCKET_CHANNEL_CAPACITY);
    let notifier = Arc::new(Notify::new());

    let tcp_write_handle = tokio::spawn(
        run_tcp_writer(route.clone(), out_socket_writer, notifier.clone(),
                       to_tcp_receiver, event_tx.clone())
    );
    let tcp_read_handle = create_bidi_tcp_client_reader(route.clone(), out_socket_reader,
                                                        notifier.clone(), from_tcp_sender, event_tx.clone()).await;

    let task_handle = tokio::spawn(
        run_udp_tcp_route(route.clone(), in_socket.clone(),
                          to_tcp_sender.clone(), buf, event_tx.clone())
    );
    let task_handle_reverse = if BiDirectional == route.flow_mode {
        let out_address = format!("{}:{}",
                                  route.in_point.socket.ip(),
                                  route.in_point.socket.port());
        Some(tokio::spawn(
            run_tcp_udp_route(route.clone(), from_tcp_receiver,
                              in_socket.clone(), out_address, event_tx.clone())
        ))
    } else { None };

    task_handle.await.expect(format!("Route {} task udp->tcp(client) panicked", route.name).as_str());
    if let Some(handle) = task_handle_reverse {
        handle.await.expect(format!("Route {} task udp<-tcp(client) panicked", route.name).as_str());
    }
    tcp_read_handle.await.expect(format!("TCP reader task failed to join for route '{}'", route.name).as_str());
    tcp_write_handle.await.expect(format!("TCP writer task failed to join for route '{}'", route.name).as_str());
}

async fn create_route_tcp_tcp_servers(route : Arc<Route>, event_tx: BcSender<Event>) {
    event_tx.send(Event::Message(format!("Creating tcp(server)-tcp(server) route '{}'", route.name))).unwrap_or_default();

    let in_socket = create_tcp_server_socket(&route.in_point).await;
    let out_socket = create_tcp_server_socket(&route.out_point).await;

    let (from_tcp_sender, from_tcp_receiver) : (MpscSender<TcpIn>, MpscReceiver<TcpIn>) = mpsc_channel(SOCKET_CHANNEL_CAPACITY);
    let (to_tcp_sender, _to_tcp_receiver) : (BcSender<Bytes>, BcReceiver<Bytes>) = bc_channel(SOCKET_CHANNEL_CAPACITY);
    let (from_tcp_sender_reverse, from_tcp_receiver_reverse) : (MpscSender<TcpIn>, MpscReceiver<TcpIn>) = mpsc_channel(SOCKET_CHANNEL_CAPACITY);
    let (to_tcp_sender_reverse, _to_tcp_receiver_reverse) : (BcSender<Bytes>, BcReceiver<Bytes>) = bc_channel(SOCKET_CHANNEL_CAPACITY);

    let handle_accept_in_socket = {
        let to_tcp_sender_reverse_opt = if BiDirectional == route.flow_mode {
            Some(to_tcp_sender_reverse.clone())
        } else { None };
        tokio::spawn(
            run_tcp_accept_loop(route.clone(), in_socket, to_tcp_sender_reverse_opt, Some(from_tcp_sender.clone()), event_tx.clone())
        )
    };
    let handle_accept_out_socket = {
        let from_tcp_sender_reverse_opt = if BiDirectional == route.flow_mode {
            Some(from_tcp_sender_reverse.clone())
        } else { None };
        tokio::spawn(
            run_tcp_accept_loop(route.clone(), out_socket, Some(to_tcp_sender.clone()), from_tcp_sender_reverse_opt, event_tx.clone())
        )
    };

    let task_handle = tokio::spawn(
        run_tcp_tcp_route(route.clone(), from_tcp_receiver, to_tcp_sender.clone(), event_tx.clone())
    );

    let task_handle_reverse = if BiDirectional == route.flow_mode {
        Some(tokio::spawn(
            run_tcp_tcp_route(route.clone(), from_tcp_receiver_reverse, to_tcp_sender_reverse.clone(), event_tx.clone())
        ))
    } else { None };

    task_handle.await.expect(format!("Route {} task (tcp->tcp) panicked", route.name).as_str());
    if let Some(handle) = task_handle_reverse {
        handle.await.expect(format!("Route {} task (tcp<-tcp) panicked", route.name).as_str());
    }
    handle_accept_in_socket.await.expect(format!("Route {} task tcp(server)->tcp(server) panicked - accept loop", route.name).as_str());
    handle_accept_out_socket.await.expect(format!("Route {} task tcp(server)->tcp(server) panicked - accept loop", route.name).as_str());
}

async fn create_route_tcp_tcp_clients(route : Arc<Route>, event_tx: BcSender<Event>) {
    event_tx.send(Event::Message(format!("Creating tcp(client)-tcp(client) route '{}'", route.name))).unwrap_or_default();

    let (in_socket_reader, in_socket_writer) = create_tcp_client_socket(&route.out_point).await.into_split();
    let (out_socket_reader, out_socket_writer) = create_tcp_client_socket(&route.out_point).await.into_split();

    let (in_to_tcp_sender, in_to_tcp_receiver) : (BcSender<Bytes>, BcReceiver<Bytes>) = bc_channel(SOCKET_CHANNEL_CAPACITY);
    let (in_from_tcp_sender, in_from_tcp_receiver) : (MpscSender<TcpIn>, MpscReceiver<TcpIn>) = mpsc_channel(SOCKET_CHANNEL_CAPACITY);
    let in_notifier = Arc::new(Notify::new());

    let (out_to_tcp_sender, out_to_tcp_receiver) : (BcSender<Bytes>, BcReceiver<Bytes>) = bc_channel(SOCKET_CHANNEL_CAPACITY);
    let (out_from_tcp_sender, out_from_tcp_receiver) : (MpscSender<TcpIn>, MpscReceiver<TcpIn>) = mpsc_channel(SOCKET_CHANNEL_CAPACITY);
    let out_notifier = Arc::new(Notify::new());

    let in_tcp_write_handle = tokio::spawn(
        run_tcp_writer(route.clone(), in_socket_writer, in_notifier.clone(),
                       in_to_tcp_receiver, event_tx.clone())
    );
    let in_tcp_read_handle = create_bidi_tcp_client_reader(route.clone(), in_socket_reader,
                                                            in_notifier.clone(), in_from_tcp_sender, event_tx.clone()).await;
    let out_tcp_write_handle = tokio::spawn(
        run_tcp_writer(route.clone(), out_socket_writer, out_notifier.clone(),
                       out_to_tcp_receiver, event_tx.clone())
    );
    let out_tcp_read_handle = create_bidi_tcp_client_reader(route.clone(), out_socket_reader,
                                                        out_notifier.clone(), out_from_tcp_sender, event_tx.clone()).await;

    let task_handle = tokio::spawn(
        run_tcp_tcp_route(route.clone(), in_from_tcp_receiver, out_to_tcp_sender.clone(), event_tx.clone())
    );

    let task_handle_reverse = if BiDirectional == route.flow_mode {
        Some(tokio::spawn(
            run_tcp_tcp_route(route.clone(), out_from_tcp_receiver, in_to_tcp_sender.clone(), event_tx.clone())
        ))
    } else { None };

    task_handle.await.expect(format!("Route {} task tcp(client)->tcp(client) panicked", route.name).as_str());
    if let Some(handle) = task_handle_reverse {
        handle.await.expect(format!("Route {} task tcp(client)<-tcp(client) panicked", route.name).as_str());
    }
    in_tcp_read_handle.await.expect(format!("TCP reader (in) task failed to join for route '{}'", route.name).as_str());
    in_tcp_write_handle.await.expect(format!("TCP writer (in) task failed to join for route '{}'", route.name).as_str());
    out_tcp_read_handle.await.expect(format!("TCP reader (out) task failed to join for route '{}'", route.name).as_str());
    out_tcp_write_handle.await.expect(format!("TCP writer (out) task failed to join for route '{}'", route.name).as_str());
}

async fn create_route_tcp_server_tcp_client(route : Arc<Route>, event_tx: BcSender<Event>) {
    event_tx.send(Event::Message(format!("Creating tcp(server)-tcp(client) route '{}'", route.name))).unwrap_or_default();

    let in_socket = create_tcp_server_socket(&route.out_point).await;
    let (out_socket_reader, out_socket_writer) = create_tcp_client_socket(&route.out_point).await.into_split();

    let (in_to_tcp_sender, _in_to_tcp_receiver) : (BcSender<Bytes>, BcReceiver<Bytes>) = bc_channel(SOCKET_CHANNEL_CAPACITY);
    let (in_from_tcp_sender, in_from_tcp_receiver) : (MpscSender<TcpIn>, MpscReceiver<TcpIn>) = mpsc_channel(SOCKET_CHANNEL_CAPACITY);

    let (out_to_tcp_sender, out_to_tcp_receiver) : (BcSender<Bytes>, BcReceiver<Bytes>) = bc_channel(SOCKET_CHANNEL_CAPACITY);
    let (out_from_tcp_sender, out_from_tcp_receiver) : (MpscSender<TcpIn>, MpscReceiver<TcpIn>) = mpsc_channel(SOCKET_CHANNEL_CAPACITY);
    let out_notifier = Arc::new(Notify::new());

    let handle_accept = {
        let to_tcp_sender_opt = if BiDirectional == route.flow_mode {
            Some(in_to_tcp_sender.clone())
        } else { None };
        tokio::spawn(
            run_tcp_accept_loop(route.clone(), in_socket, to_tcp_sender_opt, Some(in_from_tcp_sender), event_tx.clone())
        )
    };

    let out_tcp_write_handle = tokio::spawn(
        run_tcp_writer(route.clone(), out_socket_writer, out_notifier.clone(),
                       out_to_tcp_receiver, event_tx.clone())
    );
    let out_tcp_read_handle = create_bidi_tcp_client_reader(route.clone(), out_socket_reader,
                                                            out_notifier.clone(), out_from_tcp_sender, event_tx.clone()).await;

    let task_handle = tokio::spawn(
        run_tcp_tcp_route(route.clone(), in_from_tcp_receiver, out_to_tcp_sender.clone(), event_tx.clone())
    );

    let task_handle_reverse = if BiDirectional == route.flow_mode {
        Some(tokio::spawn(
            run_tcp_tcp_route(route.clone(), out_from_tcp_receiver, in_to_tcp_sender.clone(), event_tx.clone())
        ))
    } else { None };

    task_handle.await.expect(format!("Route {} task tcp(server)->tcp(client) panicked", route.name).as_str());
    if let Some(handle) = task_handle_reverse {
        handle.await.expect(format!("Route {} task tcp(server)<-tcp(client) panicked", route.name).as_str());
    }
    // TODO switch to tokio::join! to await the handles?
    // tokio::join!(
    //     handle_accept,
    //     out_tcp_read_handle,
    //     out_tcp_write_handle
    // );
    handle_accept.await.expect(format!("Route {} task tcp(server)->udp panicked - accept loop", route.name).as_str());
    out_tcp_read_handle.await.expect(format!("TCP reader (out) task failed to join for route '{}'", route.name).as_str());
    out_tcp_write_handle.await.expect(format!("TCP writer (out) task failed to join for route '{}'", route.name).as_str());
}

async fn create_route_tcp_client_tcp_server(route : Arc<Route>, event_tx: BcSender<Event>) {
    event_tx.send(Event::Message(format!("Creating tcp(client)-tcp(server) route '{}'", route.name))).unwrap_or_default();

    let (in_socket_reader, in_socket_writer) = create_tcp_client_socket(&route.in_point).await.into_split();
    let out_socket = create_tcp_server_socket(&route.out_point).await;

    let (in_from_tcp_sender, in_from_tcp_receiver) : (MpscSender<TcpIn>, MpscReceiver<TcpIn>) = mpsc_channel(SOCKET_CHANNEL_CAPACITY);
    let (in_to_tcp_sender, in_to_tcp_receiver) : (BcSender<Bytes>, BcReceiver<Bytes>) = bc_channel(SOCKET_CHANNEL_CAPACITY);
    let in_closer = Arc::new(Notify::new());

    let (out_to_tcp_sender, _out_to_tcp_receiver) : (BcSender<Bytes>, BcReceiver<Bytes>) = bc_channel(SOCKET_CHANNEL_CAPACITY);
    let (out_from_tcp_sender, out_from_tcp_receiver) : (MpscSender<TcpIn>, MpscReceiver<TcpIn>) = mpsc_channel(SOCKET_CHANNEL_CAPACITY);

    let tcp_read_handle = tokio::spawn(
        run_tcp_reader(route.clone(), in_socket_reader, in_closer.clone(),
                       in_from_tcp_sender, event_tx.clone())
    );
    let tcp_write_handle = create_bidi_tcp_client_writer(route.clone(), in_socket_writer,
                                                         in_closer.clone(),
                                                         in_to_tcp_receiver, event_tx.clone()).await;

    // accept loop for incoming TCP connections
    let handle_accept = {
        let from_tcp_sender_opt = if BiDirectional == route.flow_mode {
            Some(out_from_tcp_sender.clone())
        } else { None };
        tokio::spawn(
            run_tcp_accept_loop(route.clone(), out_socket,
                                Some(out_to_tcp_sender.clone()), from_tcp_sender_opt,
                                event_tx.clone())
        )
    };

    let task_handle = tokio::spawn(
        run_tcp_tcp_route(route.clone(), in_from_tcp_receiver, out_to_tcp_sender.clone(), event_tx.clone())
    );

    let task_handle_reverse = if BiDirectional == route.flow_mode {
        Some(tokio::spawn(
            run_tcp_tcp_route(route.clone(), out_from_tcp_receiver, in_to_tcp_sender.clone(), event_tx.clone())
        ))
    } else { None };

    task_handle.await.expect(format!("Route {} task tcp(client)->tcp(server) panicked", route.name).as_str());
    if let Some(handle) = task_handle_reverse {
        handle.await.expect(format!("Route {} task tcp(client)<-tcp(server) panicked", route.name).as_str());
    }
    handle_accept.await.expect(format!("Route {} task tcp(client)->tcp(server) panicked - accept loop", route.name).as_str());
    tcp_read_handle.await.expect(format!("TCP reader (in) task failed to join for route '{}'", route.name).as_str());
    tcp_write_handle.await.expect(format!("TCP writer (in) task failed to join for route '{}'", route.name).as_str());
}

async fn create_route_tcp_server_udp(route : Arc<Route>, event_tx: BcSender<Event>) {
    event_tx.send(Event::Message(format!("Creating tcp(server)-udp route '{}'", route.name))).unwrap_or_default();

    let in_socket = create_tcp_server_socket(&route.in_point).await;
    let out_socket = Arc::new(create_udp_socket(&route.out_point).await);

    // TODO move inside run_ functions?
    let mut buf = BytesMut::with_capacity(route.buffer_size);
    buf.resize(route.buffer_size, 0);

    let (from_tcp_sender, from_tcp_receiver) : (MpscSender<TcpIn>, MpscReceiver<TcpIn>) = mpsc_channel(SOCKET_CHANNEL_CAPACITY);
    let (to_tcp_sender, _to_tcp_receiver) : (BcSender<Bytes>, BcReceiver<Bytes>) = bc_channel(SOCKET_CHANNEL_CAPACITY);

    // accept loop for incoming TCP connections
    let handle_accept = {
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
            run_tcp_udp_route(route.clone(), from_tcp_receiver, out_socket.clone(), out_address, event_tx.clone())
        )
    };

    let task_handle_reverse = if BiDirectional == route.flow_mode {
        Some(tokio::spawn(
            run_udp_tcp_route(route.clone(), out_socket.clone(), to_tcp_sender.clone(), buf, event_tx.clone())
        ))
    } else { None };

    task_handle.await.expect(format!("Route {} task tcp(server)->udp panicked", route.name).as_str());
    if let Some(handle) = task_handle_reverse {
        handle.await.expect(format!("Route {} task tcp(server)<-udp panicked", route.name).as_str());
    }
    handle_accept.await.expect(format!("Route {} task tcp(server)->udp panicked - accept loop", route.name).as_str());
}

async fn create_route_tcp_client_udp(route : Arc<Route>, event_tx: BcSender<Event>) {
    event_tx.send(Event::Message(format!("Creating tcp(client)-udp route '{}'", route.name))).unwrap_or_default();

    let (in_socket_reader, in_socket_writer) = create_tcp_client_socket(&route.in_point).await.into_split();
    let out_socket = Arc::new(create_udp_socket(&route.out_point).await);

    // TODO move inside run_ functions?
    let mut buf = BytesMut::with_capacity(route.buffer_size);
    buf.resize(route.buffer_size, 0);

    let (from_tcp_sender, from_tcp_receiver) : (MpscSender<TcpIn>, MpscReceiver<TcpIn>) = mpsc_channel(SOCKET_CHANNEL_CAPACITY);
    let (to_tcp_sender, to_tcp_receiver) : (BcSender<Bytes>, BcReceiver<Bytes>) = bc_channel(SOCKET_CHANNEL_CAPACITY);
    let closer = Arc::new(Notify::new());

    let tcp_read_handle = tokio::spawn(
        run_tcp_reader(route.clone(), in_socket_reader, closer.clone(),
                       from_tcp_sender, event_tx.clone())
    );
    let tcp_write_handle = create_bidi_tcp_client_writer(route.clone(), in_socket_writer,
                                                         closer.clone(),
                                                         to_tcp_receiver, event_tx.clone()).await;

    let task_handle = {
        let out_address = format!("{}:{}",
                                  route.out_point.socket.ip(),
                                  route.out_point.socket.port());
        tokio::spawn(
            run_tcp_udp_route(route.clone(), from_tcp_receiver, out_socket.clone(), out_address, event_tx.clone())
        )
    };

    let task_handle_reverse = if BiDirectional == route.flow_mode {
        Some(tokio::spawn(
            run_udp_tcp_route(route.clone(), out_socket.clone(), to_tcp_sender.clone(), buf, event_tx.clone())
        ))
    } else { None };

    task_handle.await.expect(format!("Route {} task udp->tcp(client) panicked", route.name).as_str());
    if let Some(handle) = task_handle_reverse {
        handle.await.expect(format!("Route {} task udp<-tcp(client) panicked", route.name).as_str());
    }
    tcp_read_handle.await.expect(format!("TCP reader task failed to join for route '{}'", route.name).as_str());
    tcp_write_handle.await.expect(format!("TCP writer task failed to join for route '{}'", route.name).as_str());
}

async fn create_udp_socket(endpoint : &EndPoint) -> UdpSocket {
    // TODO try to bind to a specific interface/address
    // let local_addr = SocketAddrV4::new(*endpoint.socket.ip(), endpoint.socket.port());
    let local_addr = SocketAddrV4::new(Ipv4Addr::UNSPECIFIED, endpoint.socket.port());
    let socket = UdpSocket::bind(local_addr)
        .await
        .expect(format!("Error binding Endpoint to local port {}.", local_addr).as_str());

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

async fn create_tcp_client_socket(endpoint : &EndPoint) -> TcpStream {
    let remote_addr = SocketAddrV4::new(*endpoint.socket.ip(), endpoint.socket.port());
    TcpStream::connect(remote_addr).await
        .expect(format!("Error connecting to remote TCP address {:?}", remote_addr).as_str())
}

async fn create_tcp_server_socket(endpoint : &EndPoint) -> TcpListener {
    let listener_addr = SocketAddrV4::new(Ipv4Addr::UNSPECIFIED, endpoint.socket.port());
    TcpListener::bind(listener_addr).await
        .expect(format!("Error binding TCP listener socket for {:?}", listener_addr).as_str())
}

/// Create task for a bidirectional route using a TCP client connection, reader half.
/// Creates a dummy task for unidirectional routes.
async fn create_bidi_tcp_client_reader(route : Arc<Route>, socket_reader: OwnedReadHalf, closer: Arc<Notify>, from_tcp_sender: MpscSender<TcpIn>, event_tx: BcSender<Event>) -> JoinHandle<()> {
    if BiDirectional == route.flow_mode {
        tokio::spawn(
            run_tcp_reader(route.clone(), socket_reader, closer,
                           from_tcp_sender, event_tx.clone())
        )
    } else {
        tokio::spawn(
            run_tcp_reader_dummy(socket_reader, closer)
        )
    }
}

/// Create task for a bidirectional route using a TCP client connection, writer half.
/// Creates a dummy task for unidirectional routes.
async fn create_bidi_tcp_client_writer(route : Arc<Route>, socket_writer: OwnedWriteHalf, closer: Arc<Notify>, to_tcp_receiver: BcReceiver<Bytes>, event_tx: BcSender<Event>) -> JoinHandle<()> {
    if BiDirectional == route.flow_mode {
        tokio::spawn(run_tcp_writer(route.clone(), socket_writer, closer,
                       to_tcp_receiver, event_tx.clone()))
    } else {
        tokio::spawn(run_tcp_writer_dummy(socket_writer, closer))
    }
}

/// Runs the accept loop for TCP server sockets.
/// Spawns tasks for established connections based on the route configuration.
async fn run_tcp_accept_loop(route : Arc<Route>, socket : TcpListener, to_tcp_sender_opt: Option<BcSender<Bytes>>, from_tcp_receiver_opt: Option<MpscSender<TcpIn>>, event_tx: BcSender<Event>) {
    let sem = Arc::new(Semaphore::new(route.max_connections));

    loop {
        if let Ok(permit) = sem.clone().acquire_owned().await {
            let (stream, peer_addr) = socket.accept().await
                .expect(format!("Error establishing incoming TCP connection for route '{}'.", route.name).as_str());
            event_tx.send(Event::SocketConnected(Scheme::TCP, peer_addr))
                .expect("Error sending Event::SocketConnected through route event channel.");
            let (reader, writer) = stream.into_split();
            let notifier = Arc::new(Notify::new());

            let (write_handle, read_handle) =
            match (&to_tcp_sender_opt, &from_tcp_receiver_opt) {
                (Some(ref to_tcp_sender), Some(ref from_tcp_receiver)) => {

                    (tokio::spawn(
                        run_tcp_writer(route.clone(),
                                       writer, notifier.clone(),
                                       to_tcp_sender.subscribe(),
                                       event_tx.clone())
                    ),
                     tokio::spawn(
                        run_tcp_reader(route.clone(),
                                       reader, notifier.clone(),
                                       from_tcp_receiver.clone(),
                                       event_tx.clone())
                    ))
                }
                (Some(ref to_tcp_sender), None) => {
                    (tokio::spawn(
                        run_tcp_writer(route.clone(),
                                       writer, notifier.clone(),
                                       to_tcp_sender.subscribe(),
                                       event_tx.clone())
                    ),tokio::spawn(
                        run_tcp_reader_dummy(reader, notifier.clone())
                    ))
                }
                (None, Some(ref from_tcp_receiver)) => {
                    (tokio::spawn(
                        run_tcp_writer_dummy(writer, notifier.clone())
                    ),
                     tokio::spawn(
                        run_tcp_reader(route.clone(),
                                       reader, notifier.clone(),
                                       from_tcp_receiver.clone(),
                                       event_tx.clone())
                    ))
                }
                (None, None) => {break;}
            };
            // task to wait for task joins
            tokio::spawn(async move {
                read_handle.await.unwrap();
                write_handle.await.unwrap();
                drop(permit); // Permits should be dropped after a spawned socket task has finished.
            });
        } else {
            // TODO
            event_tx.send(
                Event::Error(
                    format!("Error acquiring a permit for TCP connection on socket {} for route '{}'",
                            socket.local_addr().unwrap(),
                            route.name)))
                .unwrap_or_default();
        }
    }
}

async fn run_tcp_writer(route : Arc<Route>, mut writer: OwnedWriteHalf, closer: Arc<Notify>,
                        mut data_channel : BcReceiver<Bytes>, event_tx: BcSender<Event>) {
    loop {
        tokio::select! {
            _ = closer.notified() => {
                break;
            }
            received = data_channel.recv() => {
                if let Ok(out_buf) = received {
                    match writer.write(&out_buf[..]).await {
                        Ok(0) => { break; }
                        Ok(bytes_send) => {
                            if let Err(msg) = event_tx.send(Event::StatBytesSend(0, bytes_send)) {
                                trace!("Error sending runtime send statistics for route '{}' through channel: {}", route.name, msg);
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
    }
}

/// Task that holds on to (e.g., prevents dropping) a OwnedWriteHalf of a TcpStream
/// until the connection closes, when indicated by the Notify-channel
async fn run_tcp_writer_dummy(mut _writer: OwnedWriteHalf, closer : Arc<Notify>) {
    // just wait for the connection to close
    closer.notified().await;
}


async fn run_tcp_reader(route : Arc<Route>, mut reader: OwnedReadHalf, closer : Arc<Notify>,
                        data_channel : MpscSender<TcpIn>, event_tx: BcSender<Event>) {
    let mut buf = BytesMut::with_capacity(route.buffer_size);
    buf.resize(route.buffer_size, 0);
    loop {
        match reader.read(&mut buf).await {
            Ok(0) => {
                closer.notify_one();
                event_tx.send(Event::SocketDisconnected(
                    Scheme::TCP, reader.peer_addr().expect("Should be Ok")))
                    .expect("Error sending Event::SocketDisconnected through event channel.");
                break;
            }
            Ok(bytes_received) => {
                // FIXME now always copying the whole buf
                // let buf_to_send = Bytes::copy_from_slice(&buf[..bytes_received]);
                let buf_to_send = BytesMut::from(&buf[..bytes_received]);
                if let Err(msg) = event_tx.send(Event::StatBytesReceived(0, bytes_received)) {
                    event_tx.send(Event::Error(
                        format!("Error sending runtime receive statistics for route '{}' through channel: {}", route.name, msg)
                    )).unwrap_or_default();
                }
                data_channel.send((buf_to_send, reader.peer_addr().unwrap())).await.expect("Error sending received data through mpsc channel.");
            }
            Err(err) => {
                event_tx.send(Event::Error(format!("Error while receiving data via tcp: {}", err))).unwrap_or_default();
                break;
            }
        }
    }
}

/// Task that holds on to (e.g., prevents dropping) a OwnedReadHalf of a TcpStream
/// until the connection closes, as indicated by receiving 0 bytes.
/// Notifies the OwnedWriteHalf task of the closing of the connection.
async fn run_tcp_reader_dummy(mut reader: OwnedReadHalf, closer : Arc<Notify>) {
    let buffer_size = 1024;
    let mut buf = BytesMut::with_capacity(buffer_size);
    buf.resize(buffer_size, 0);
    loop {
        match reader.read(&mut buf).await {
            Ok(0) => {
                closer.notify_one();
                break;
            }
            _ => { }
        }
    }
}

fn message_is_from_own_host(host: IpAddr, received_from : IpAddr) -> bool {
    return host == received_from;
}

async fn run_udp_udp_route(route : Arc<Route>,
                           in_socket : Arc<UdpSocket>, out_socket : Arc<UdpSocket>, out_address : String,
                           mut buf: BytesMut,
                           event_tx: BcSender<Event>) {
    loop {
        let (bytes_received, from_address) = in_socket.recv_from(&mut buf).await.expect("Error receiving from incoming Endpoint.");

        if route.block_host &&
            message_is_from_own_host(route.in_point.interface, from_address.ip()) { continue }

        // collect statistics for rx
        if let Err(msg) = event_tx.send(Event::StatBytesReceived(0, bytes_received)) {
            trace!("Error sending runtime receive statistics for route '{}' through channel: {}", route.name, msg);
        }

        if passes_filters(&buf, route.filters.iter(), &&*&(bytes_received, from_address)) {
            let send_buf = apply_transformers(&buf, route.transformers.iter(), &&(bytes_received, from_address));

            match out_socket.send_to(&send_buf[..],
                                     out_address.as_str()).await {
                Ok(bytes_send) => {
                    // collect statistics for tx
                    if let Err(msg) = event_tx.send(Event::StatBytesSend(0, bytes_send)) {
                        trace!("Error sending runtime send statistics for route '{}' through channel: {}", route.name, msg);
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
    };
}

async fn run_udp_tcp_route(route : Arc<Route>,
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
        }

        if passes_filters(&buf, route.filters.iter(), &&(bytes_received, from_address)) {
            let send_buf = apply_transformers(&buf, route.transformers.iter(), &&(bytes_received, from_address));
            let bytes_produced = send_buf.len();

            let _num_receivers = out_sender.send(send_buf.clone()).expect("Error forwarding outgoing data to sending sockets");

            if let Err(msg) = event_tx.send(Event::StatBytesSend(0, bytes_produced)) {
                trace!("Error sending runtime receive statistics for route '{}' through channel: {}", route.name, msg);
            }
        }
    }
}

async fn run_tcp_udp_route(route : Arc<Route>, mut in_receiver: MpscReceiver<TcpIn>, out_socket : Arc<UdpSocket>, out_address : String, event_tx: BcSender<Event>) {
    loop {
        let (bytes, from_address) = in_receiver.recv().await.expect("Error receiving from incoming endpoint channel.");

        // collect statistics for rx
        if let Err(msg) = event_tx.send(Event::StatBytesReceived(0, bytes.len())) {
            trace!("Error sending runtime receive statistics for route '{}' through channel: {}", route.name, msg);
        }

        if passes_filters(&bytes, route.filters.iter(), &&*&(bytes.len(), from_address)) {
            let send_buf = apply_transformers(&bytes, route.transformers.iter(), &&(bytes.len(), from_address));

            match out_socket.send_to(&send_buf[..], out_address.as_str()).await {
                Ok(bytes_send) => {
                    // collect statistics for tx
                    if let Err(msg) = event_tx.send(Event::StatBytesSend(0, bytes_send)) {
                        trace!("Error sending runtime send statistics for route '{}' through channel: {}", route.name, msg);
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
}

async fn run_tcp_tcp_route(route : Arc<Route>, mut in_receiver: MpscReceiver<TcpIn>, out_sender : BcSender<Bytes>, event_tx: BcSender<Event>) {
    loop {
        let (bytes, from_address) = in_receiver.recv().await.expect("Error receiving from incoming endpoint channel.");

        // collect statistics for rx
        if let Err(msg) = event_tx.send(Event::StatBytesReceived(0, bytes.len())) {
            trace!("Error sending runtime receive statistics for route '{}' through channel: {}", route.name, msg);
        }

        if passes_filters(&bytes, route.filters.iter(), &&*&(bytes.len(), from_address)) {
            let send_buf = apply_transformers(&bytes, route.transformers.iter(), &&(bytes.len(), from_address));
            let bytes_produced = send_buf.len();

            let _bytes_out = out_sender.send(send_buf).expect("Error forwarding outgoing data to sending sockets");

            if let Err(msg) = event_tx.send(Event::StatBytesSend(0, bytes_produced)) {
                trace!("Error sending runtime receive statistics for route '{}' through channel: {}", route.name, msg);
            }
        }
    }
}

async fn start_hub(hub: Arc<Hub>, event_tx: BcSender<Event>) {
    // let listeners : Vec<TcpListener> = hub.connections.iter()
    //     .map(|endpoint| create_tcp_server_socket(endpoint))
    //     .collect();
    //
    // if listeners.is_empty() {
    //     return;
    // }

    // TODO currently only supports one TCP socket, to accommodate multiple connections. We take the first if > 1 is configured
    let listener = create_tcp_server_socket(hub.connections
        .first()
        .expect(format!("Should be at least one hub connection configured for hub {}", hub.name).as_str())
    ).await;

    let (to_tcp_sender, _to_tcp_receiver) : (BcSender<TcpHubOut>, BcReceiver<TcpHubOut>) = bc_channel(SOCKET_CHANNEL_CAPACITY);
    let (from_tcp_sender, from_tcp_receiver) : (MpscSender<TcpIn>, MpscReceiver<TcpIn>) = mpsc_channel(SOCKET_CHANNEL_CAPACITY);

    let handle_accept = tokio::spawn(run_hub_accept_loop(hub.clone(), listener, to_tcp_sender.clone(), from_tcp_sender, event_tx.clone()));
    let handle_hub = tokio::spawn(run_hub(hub.clone(), from_tcp_receiver, to_tcp_sender.clone(), event_tx.clone()));

    handle_hub.await.expect(format!("Hub {} task panicked", hub.name).as_str());
    handle_accept.await.expect(format!("Hub {} task panicked - accept loop", hub.name).as_str());
}

async fn run_hub(hub: Arc<Hub>, mut in_receiver: MpscReceiver<TcpIn>, out_sender : BcSender<TcpHubOut>, event_tx: BcSender<Event>) {
    loop {
        let (bytes, from_address) = in_receiver.recv().await.expect("Error receiving from incoming endpoint channel.");

        if let Err(msg) = event_tx.send(Event::StatBytesReceived(0, bytes.len())) {
            trace!("Error sending runtime receive statistics for hub '{}' through channel: {}", hub.name, msg);
        }

        if passes_filters(&bytes, hub.filters.iter(), &&*&(bytes.len(), from_address)) {
            let send_buf = apply_transformers(&bytes, hub.transformers.iter(), &&(bytes.len(), from_address));
            let bytes_produced = send_buf.len();

            let _bytes_out = out_sender.send((send_buf, from_address)).expect("Error forwarding outgoing data to sending sockets");

            if let Err(msg) = event_tx.send(Event::StatBytesSend(0, bytes_produced)) {
                trace!("Error sending runtime receive statistics for hub '{}' through channel: {}", hub.name, msg);
            }
        }
    }
}

async fn run_hub_accept_loop(hub : Arc<Hub>, socket : TcpListener, to_tcp_sender: BcSender<TcpHubOut>, from_tcp_sender: MpscSender<TcpIn>, event_tx: BcSender<Event>) {
    let sem = Arc::new(Semaphore::new(hub.max_connections));

    loop {
        if let Ok(permit) = sem.clone().acquire_owned().await {
            let (stream, peer_addr) = socket.accept().await
                .expect(format!("Error establishing incoming TCP connection for hub '{}'.", hub.name).as_str());
            event_tx.send(Event::SocketConnected(Scheme::TCP, peer_addr))
                .expect("Error sending Event::SocketConnected through hub event channel.");
            let (reader, writer) = stream.into_split();
            let notifier = Arc::new(Notify::new());

            let read_handle = tokio::spawn(run_hub_reader(hub.clone(), reader, notifier.clone(), from_tcp_sender.clone(), event_tx.clone()));
            let write_handle = tokio::spawn(run_hub_writer(hub.clone(), writer, notifier.clone(), to_tcp_sender.subscribe(), event_tx.clone()));

            // task to wait for task joins
            tokio::spawn(async move {
                read_handle.await.unwrap();
                write_handle.await.unwrap();
                drop(permit); // Permits should be dropped after a spawned socket task has finished.
            });
        } else {
            // TODO
            event_tx.send(
                Event::Error(
                    format!("Error acquiring a permit for TCP connection on socket {} for route '{}'",
                            socket.local_addr().unwrap(),
                            hub.name)))
                .unwrap_or_default();
        }
    }
}

async fn run_hub_reader(hub: Arc<Hub>, mut reader: OwnedReadHalf, closer: Arc<Notify>, data_channel: MpscSender<TcpIn>, event_tx: BcSender<Event>) {
    let mut buf = BytesMut::with_capacity(hub.buffer_size);
    buf.resize(hub.buffer_size, 0);

    loop {
        match reader.read(&mut buf).await {
            Ok(0) => {
                closer.notify_one();
                event_tx.send(Event::SocketDisconnected(
                    Scheme::TCP, reader.peer_addr().expect("Should be Ok.")))
                    .expect("Error sending Event::SocketDisconnected through hub event channel.");
                break;
            }
            Ok(bytes_received) => {
                // FIXME now always copying the whole buf
                // let buf_to_send = Bytes::copy_from_slice(&buf[..bytes_received]);
                let buf_to_send = BytesMut::from(&buf[..bytes_received]);
                // TODO adapt event to include reader.peer_addr or some id to identify the peer; also adapt for routes
                if let Err(msg) = event_tx.send(Event::StatBytesReceived(0, bytes_received)) {
                    event_tx.send(Event::Error(
                        format!("Error sending runtime receive statistics for hub '{}' through channel: {}", hub.name, msg)
                    )).unwrap_or_default();
                }
                data_channel.send((buf_to_send, reader.peer_addr().expect("TCP Peer address should be Ok")))
                    .await.expect("Error sending received data through mpsc channel.");
            }
            Err(err) => {
                event_tx.send(Event::Error(format!("Error while receiving data via tcp: {}", err))).unwrap_or_default();
                break;
            }
        }
    }
}

async fn run_hub_writer(hub: Arc<Hub>, mut writer: OwnedWriteHalf, closer: Arc<Notify>, mut data_channel: BcReceiver<TcpHubOut>, event_tx: BcSender<Event>) {
    loop {
        tokio::select! {
            _ = closer.notified() => {
                break;
            }
            received = data_channel.recv() => {
                if let Ok((out_buf, from_address)) = received {
                    if writer.peer_addr().expect("TcpWriter peer_addr Should be Ok") == from_address { continue; }
                    match writer.write(&out_buf[..]).await {
                        Ok(0) => { break; }
                        Ok(bytes_send) => {
                            if let Err(msg) = event_tx.send(Event::StatBytesSend(0, bytes_send)) {
                                trace!("Error sending runtime send statistics for route '{}' through channel: {}", hub.name, msg);
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
    }
}