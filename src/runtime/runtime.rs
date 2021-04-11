use std::sync::mpsc::Sender as BlockingSender;

use std::net::Ipv4Addr;

use tokio::time::{Duration, interval};
use tokio::sync::mpsc::Receiver;

use crossterm::event::{KeyCode, KeyEvent};

use crate::events::Command;

pub struct Route {
    in_point : EndPoint,
    out_point : EndPoint,
    // filters_in : Vec<Filter>,
    // filters_out : Vec<Filter>,
    // transforms : Vec<Transform>
}

pub struct EndPoint {
    ip : Ipv4Addr,
    port : i16,
    protocol : ProtocolType,
}

pub enum ProtocolType {
    UDP,
    TCP,
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

pub async fn start_main_task(mut rt_rx: Receiver<Command>, to_gui_tx : BlockingSender<Command>) {
    // spawn gateway main task
    let _main_task = tokio::spawn(start_route(to_gui_tx) );

    while let Some(command) = rt_rx.recv().await {
        match command {
            Command::Quit => {
                break;
            }
            _ => {}
        }
    }
}

async fn start_route(gui_tx : BlockingSender<Command>) {
    let mut interval = interval(Duration::from_secs(2));

    loop {
        interval.tick().await;
        gui_tx.send(Command::None); // no-op for now, to show a sign of life
    }
}