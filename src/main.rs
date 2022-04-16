pub mod events;
pub mod gui;
pub mod runtime;
pub mod model;

use log::{info};

extern crate clap;
use clap::{Arg, App, ArgMatches};

use toml;

use tokio::runtime::{Builder as rtBuilder};
use tokio::sync::broadcast::{channel, Sender};

use crossterm::{
    event::{self, Event as CEvent},
    terminal::enable_raw_mode
};

use std::io::{Error, ErrorKind, Read};
use std::thread::{Builder as thrBuilder, JoinHandle};
use std::fs::File;
use std::convert::TryFrom;

use crate::gui::{start_gui};
use crate::runtime::{start_runtime};
use crate::events::Command;
use crate::model::constants::*;
use crate::model::arguments::*;
use crate::model::config::*;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let arg_matches = get_cli_arguments();

    // Setup logging & RUST_LOG from args; RUST_LOG takes precedence over the -log argument; defaults to "info"
    if std::env::var("RUST_LOG").is_err() {
        let log_level = if let Some(log_level) = arg_matches.value_of("log") {
            log_level
        } else {
            "info"
        };
        std::env::set_var("RUST_LOG", format!("{},mio=info", log_level));
    }
    // enable console logging
    tracing_subscriber::fmt::init();

    get_config(&arg_matches)?.make_new_current();
    let mode = Config::current().mode;

    // TODO make channel capacities a setting
    let (command_tx, command_rx) = channel(COMMAND_CHANNEL_CAPACITY);
    let (event_tx, _event_rx) = channel(EVENT_CHANNEL_CAPACITY);

    let input_thread = start_input_thread(&mode, command_tx.clone());

    let gui_thread = match mode {
        Mode::Interactive => {
            start_gui(
                command_tx.subscribe(),
                command_tx.clone(),
                event_tx.subscribe())
        }
        Mode::Headless => Err(Error::new(ErrorKind::Other, "GUI not available in Headless mode"))
    };

    // tokio runtime runs in the main thread
    let runtime = rtBuilder::new_multi_thread()
        .enable_io()
        .enable_time()
        .build().unwrap();
    let _guard = runtime.enter();
    runtime.block_on( async { start_runtime(command_rx, event_tx).await});
    runtime.shutdown_background();

    if Mode::Interactive == mode {
        if let Ok(gui_handle) = gui_thread {
            gui_handle.join().expect("Could not join on the associated thread: GUI");
        }
    }
    if let Ok(input_handle) = input_thread {
        input_handle.join().expect("Could not join on the associated thread: Input");
    }

    Ok(())
}

fn get_cli_arguments() -> ArgMatches<'static> {
    // Read config: we expect a .toml file with the config (required for now).
    // TODO Or perhaps later most settings as separate arguments.
    App::new("sim-gateway")
        .version("0.1.0")
        .author("Zeeger Lubsen <zeeger@lubsen.eu>")
        .about("Gateway component for routing, filtering and transforming simulation protocols")
        .arg(Arg::with_name("config")
            .short("c")
            .long("config")
            .value_name("FILE")
            .help("Sets a custom config file (.toml)")
            .required(true)
            .takes_value(true))
        .arg(Arg::with_name("interactive")
            .short("i")
            .long("interactive")
            .required(false)
            .help("Enable interactive CLI mode"))
        .arg(Arg::with_name("log")
            .short("l")
            .long("log")
            .value_name("LOG_LEVEL")
            .help("Sets the log level (trace, debug, info, warn, error)")
            .required(false)
            .takes_value(true))
        .get_matches()
}

// TODO possibly convert to config_rs crate
fn get_config(arg_matches : &ArgMatches) -> Result<Config, Box<dyn std::error::Error>> {
    let config_from_args = if let Some(config_file) = arg_matches.value_of("config") {
        info!("Read arguments from file {}", config_file);
        let mut file = File::open(config_file)?;
        let mut buffer = String::new();
        file.read_to_string(&mut buffer)?;
        // TODO return / exit with clean error message

        let arguments : Arguments = toml::from_str(buffer.as_str())?;
        let config = Config::try_from(&arguments)?;

        Some(config)
    } else { None };

    let config_from_cli = if arg_matches.is_present("interactive") {
        Some(Config {
            mode: Mode::Interactive,
            routes: Vec::new(),
            hubs: Vec::new(),
        })
    } else { None };

    let config = merge_configs(config_from_args, config_from_cli);

    Ok(config)
}

fn start_input_thread(mode : &Mode, command_tx_input : Sender<Command>) -> Result<JoinHandle<()>, std::io::Error> {
    if Mode::Interactive == *mode {
        enable_raw_mode().expect("Failed to set raw mode");
    }
    let input_builder = thrBuilder::new().name("Input".into());

    let mut command_rx_input = command_tx_input.subscribe();

    input_builder.spawn(move || {
        let poll_rate = std::time::Duration::from_millis(INPUT_POLL_RATE_MS);

        loop {
            // poll for user inputs
            if event::poll(poll_rate).expect("Key event polling error") {
                if let CEvent::Key(key) = event::read().expect("Key event read error") {
                    let command = Command::from(key);
                    command_tx_input.send(command).expect("Send key event error");
                }
            }
            // handle incoming commands
            if let Ok(command) = command_rx_input.try_recv() {
                match command {
                    Command::Quit => break,
                    Command::Key('q') => {
                        if Config::current().mode == Mode::Headless {
                            let _result = command_tx_input.send(Command::Quit);
                        }
                    },
                    _ => {}
                }
            }
        }
    })
}