use std::sync::mpsc::Sender as BlockingSender;

use std::net::Ipv4Addr;

use tokio::time::{Duration, interval};
use tokio::sync::mpsc::Receiver;

use crossterm::event::{KeyCode, KeyEvent};

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

pub async fn start_main_task(mut to_rt_rx: Receiver<KeyEvent>, shutdown_tx : BlockingSender<bool>) {
    // spawn gateway main task
    let _main_task = tokio::spawn(start_route() );

    while let Some(key) = to_rt_rx.recv().await {
        match key.code {
            KeyCode::Char('q') => {
                println!("rt will quit");
                shutdown_tx.send(true).expect("Shutdown signal send failure");
                break;
            }
            _ => {}
        }
    }
}

async fn start_route() {
    let mut interval = interval(Duration::from_secs(2));

    loop {
        interval.tick().await;
        println!("I'm the networking task.");
    }
}