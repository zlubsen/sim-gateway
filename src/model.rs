pub mod arguments;
pub mod config;

// pub use config::*;
// pub use arguments::*;

use arguments::*;
use config::*;
use log::debug;

// TODO where to get default values from?
pub fn from_arguments(args : arguments::Arguments) -> config::Config {
    let mode = if let Some(mode_arg) = args.mode { Mode::from_str(mode_arg).unwrap() };
    let routes = args.routes.iter().map(|route_spec| {
        parse_endpoint(route_spec.in_point.as_str());
    } ).filter(|&option|option.is_some());
}

fn parse_endpoint(spec: &str) -> Option<EndPoint> {
    let parts = spec.split('/').collect();
    debug!("{:?}", parts);

    None
}