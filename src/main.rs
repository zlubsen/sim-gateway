pub mod events;
pub mod gui;
pub mod runtime;
pub mod model;

use log::{error, warn, info, debug, trace};
use env_logger;

extern crate clap;
use clap::{Arg, App, ArgMatches};

use toml;

use tokio::runtime::{Builder as rtBuilder};
use tokio::sync::broadcast::channel;

use crossterm::{
    event::{self, Event as CEvent},
    terminal::enable_raw_mode
};

use std::io::{Error, ErrorKind, Read};
use std::thread::{Builder as thrBuilder};
use std::fs::File;
use std::convert::TryFrom;

use crate::gui::{start_gui, Settings};
use crate::runtime::{start_runtime_task};
use crate::events::Command;
use crate::model::arguments::*;
use crate::model::config::*;

const INPUT_POLL_RATE : u64 = 100;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    env_logger::init();

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

    get_config(&arg_matches).unwrap().make_current();

    let mode = Config::current().mode;

    // // TODO replace with crossbeam channels?
    // let (to_rt_tx, rt_rx) = tokio_async_channel(10);
    // let gui_to_rt_tx = to_rt_tx.clone();
    // let (to_gui_tx, gui_rx) = std_sync_channel();
    // let rt_to_gui_tx = to_gui_tx.clone();

    // let (command_tx, command_rx) = bounded(10);
    // let (data_tx, data_rx) = bounded(100);
    let (command_tx, command_rx) = channel(10);
    let (data_tx, data_rx) = channel(100);

    let input_thread = {
        if Mode::Interactive == mode {
            enable_raw_mode().expect("Failed to set raw mode");
        }
        let input_builder = thrBuilder::new().name("Input".into());

        // let command_tx_input = command_tx.clone();
        // let command_rx_input = command_rx.clone();
        let command_tx_input = command_tx.clone();
        let mut command_rx_input = command_tx_input.subscribe();
        input_builder.spawn(move || {
            let poll_rate = std::time::Duration::from_millis(INPUT_POLL_RATE);

            loop {
                // poll for user inputs
                if event::poll(poll_rate).expect("Key event polling error") {
                    if let CEvent::Key(key) = event::read().expect("Key event read error") {
                        trace!("user key event: {:?}", key);
                        let command = Command::from(key);
                        trace!("command to send: {:?}", command);
                        command_tx_input.send(command).expect("Send key event error");
                    }
                }
                // handle incoming commands
                if let Ok(command) = command_rx_input.try_recv() {
                    trace!("input recv command: {:?}", command);
                    match command {
                        Command::Quit => break,
                        Command::Key('q') => {
                            if mode == Mode::Headless {
                                let _result = command_tx_input.send(Command::Quit);
                            }
                        },
                        _ => {}
                    }
                }
            }
        })
    };

    // let command_rx_gui = command_rx.clone();
    // let data_rx_gui = data_rx.clone();
    let command_rx_gui = command_tx.subscribe();
    let command_tx_gui = command_tx.clone();
    let data_rx_gui = data_tx.subscribe();
    let gui_thread = match Config::current().mode {
        Mode::Interactive => {
            start_gui(
                command_rx_gui,
                command_tx_gui,
                data_rx_gui)
        }
        Mode::Headless => Err(Error::new(ErrorKind::Other, "GUI not available in Headless mode"))
    };

    let _runtime_thread = {
        let runtime = rtBuilder::new_multi_thread()
            .enable_io()
            .enable_time()
            // .on_thread_start(|| {
            //     println!("Async runtime thread started.");
            // })
            .build().unwrap();
        let _guard = runtime.enter();

        let rt_builder = thrBuilder::new().name("Runtime".into());
        rt_builder.spawn(move || {
            runtime.block_on(start_runtime_task(command_rx, data_tx));
            runtime.shutdown_background();
        })
    };

    if _runtime_thread.is_ok() {
        _runtime_thread.unwrap().join().expect("Could not join on the associated thread, Runtime");
    }
    if Mode::Interactive == mode {
        if let Ok(gui) = gui_thread {
            gui.join().expect("Could not join on the associated thread, GUI");
        }
    }
    if let Ok(input) = input_thread {
        input.join().expect("Could not join on the associated thread, Input");
    }

    Ok(())
}

fn get_config(arg_matches : &ArgMatches) -> Result<Config, Box<dyn std::error::Error>> {
    let config_from_args = if let Some(config_file) = arg_matches.value_of("config") {
        info!("Read arguments from file {}", config_file);
        let mut file = File::open(config_file)?;
        let mut buffer = String::new();
        file.read_to_string(&mut buffer)?;
        // TODO return / exit with clean error message
        let arguments : Arguments = toml::from_str(buffer.as_str())?;
        trace!("Args:\n{:?}", arguments);

        let config = Config::try_from(&arguments)?;
        trace!("Config:\n{:?}", config);

        Some(config)
    } else { None };

    let config_from_cli = if arg_matches.is_present("interactive") {
        Some(Config {
            mode: Mode::Interactive,
            routes: Vec::new(),
        })
    } else { None };

    let config = merge_configs(config_from_args, config_from_cli);
    trace!("Merged config:\n{:?}", config);

    Ok(config)
}