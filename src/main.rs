pub mod events;
pub mod gui;
pub mod runtime;

extern crate clap;
use clap::{Arg, App};

use tokio::runtime::{Builder as rtBuilder};
use tokio::sync::mpsc::channel as tokio_async_channel;

use crossterm::{
    event::{self, Event as CEvent},
    terminal::enable_raw_mode
};

use std::io::{Error, ErrorKind};
use std::thread::{Builder as thrBuilder};
use std::sync::mpsc::channel as std_sync_channel;

use crate::gui::{start_gui, Settings};
use crate::runtime::{start_main_task, Route};
use crate::events::Command;

struct GateConfig {
    mode : Mode,
    routes : Vec<Route>,
}

#[derive(Copy, Clone, Debug, PartialEq)]
enum Mode {
    Interactive,
    Headless,
}

impl Default for Mode {
    fn default() -> Self { Mode::Interactive }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let runtime = rtBuilder::new_multi_thread()
        .enable_io()
        .enable_time()
        .on_thread_start(|| {
            println!("Async runtime thread started.");
        }).build().unwrap();
    let _guard = runtime.enter();

    // Read config: we expect a .toml file with the config (required for now).
    // TODO Or perhaps later most settings as separate arguments.
    // TODO -v verbosity level
    let arg_matches = App::new("sim-gateway")
        .version("0.1.0")
        .author("Zeeger Lubsen <zeeger@lubsen.eu>")
        .about("Gateway component for routing, filtering and transforming simulation protocols")
        .arg(Arg::with_name("config")
            .short("c")
            .long("config")
            .value_name("FILE")
            .help("Sets a custom config file (.toml)")
            // .required(true)
            .takes_value(true))
        .arg(Arg::with_name("interactive")
            .short("i")
            .long("interactive")
            .help("Enable interactive CLI mode"))
        .get_matches();

    // TODO make config mandatory
    // if let Some(config) = arg_matches.value_of("config").unwrap() {
    //
    // }

    let config = GateConfig {
        mode: if arg_matches.is_present("interactive") { Mode::Interactive } else { Mode::Headless },
        routes: vec![]
    };

    const INPUT_POLL_RATE : u64 = 100;
    const INPUT_RECV_TIMEOUT : u64 = 100;

    let (to_rt_tx, to_rt_rx) = tokio_async_channel(10);
    let (to_gui_tx, to_gui_rx) = std_sync_channel();
    let (shutdown_tx, shutdown_rx) = std_sync_channel();
    let rt_to_gui_tx = to_gui_tx.clone();

    let input_thread = {
        let mode = config.mode;
        if mode == Mode::Interactive {
            enable_raw_mode().expect("Failed to set raw mode");
        }
        let input_builder = thrBuilder::new().name("Input".into());

        input_builder.spawn(move || {
            let poll_rate = std::time::Duration::from_millis(INPUT_POLL_RATE);
            let receive_rate = std::time::Duration::from_millis(INPUT_RECV_TIMEOUT);

            loop {
                if event::poll(poll_rate).expect("Key event polling error") {
                    if let CEvent::Key(key) = event::read().expect("Key event read error") {
                        if mode == Mode::Interactive {
                            to_gui_tx.send(Command::from(key)).expect("Send key event to GUI error");
                        }
                        to_rt_tx.blocking_send(Command::from(key)).expect("Send key event to runtime error");
                    }
                }
                if shutdown_rx.recv_timeout(receive_rate).is_ok() {
                    println!("Shutdown signal received.");
                    break;
                }
            }
        })
    };

    let gui_thread = match config.mode {
        Mode::Interactive => {
            start_gui(
                Settings {
                placeholder: 0
            },
            to_gui_rx)
        }
        Mode::Headless => Err(Error::new(ErrorKind::Other, "GUI not available in Headless mode"))
    };

    let _runtime_thread = {
        let rt_builder = thrBuilder::new().name("Runtime".into());
        rt_builder.spawn(move || {
            runtime.block_on(start_main_task(to_rt_rx, rt_to_gui_tx, shutdown_tx));
            runtime.shutdown_background();
        })
    };

    if _runtime_thread.is_ok() {
        _runtime_thread.unwrap().join().expect("Could not join on the associated thread, Runtime");
    }
    if Mode::Interactive == config.mode {
        if let Ok(gui) = gui_thread {
            gui.join().expect("Could not join on the associated thread, GUI");
        }
    }
    if let Ok(input) = input_thread {
        input.join().expect("Could not join on the associated thread, Input");
    }

    Ok(())
}