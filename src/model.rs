pub mod arguments;
pub mod config;

// pub use config::*;
// pub use arguments::*;

use arguments::*;
use config::*;
use log::debug;

// TODO where to get default values from?
pub fn from_arguments(args : arguments::Arguments) -> config::Config {
    let mode = match args.mode {
        Some(mode_arg) => Mode::from(mode_arg.as_str()),
        None => Mode::default(),
    };
    let routes = args.routes.iter()
        .map(|route_spec| { parse_route(route_spec) } )
        .filter(|option|option.is_some())
        .map(|some|some.unwrap())
        .collect();

    Config {
        mode,
        routes,
    }
}

fn parse_route(spec : &RouteSpec) -> Option<Route> {
    // TODO input validation
    Some(Route{
        name : String::from(spec.name.as_str()),
        in_point: parse_endpoint(spec.in_point.as_str()).unwrap(),
        out_point: parse_endpoint(spec.out_point.as_str()).unwrap(),
        buffer_size: spec.buffer_size.unwrap_or(32768) as usize,
        flow_mode: FlowMode::UniDirectional,
    })
}

fn parse_endpoint(spec: &str) -> Result<EndPoint, &str> {
    // TODO input validation
    let (scheme, socket_address) = match spec.split_once("://") {
        Some((sch, sock)) => (Scheme::from(sch), sock),
        None => (Scheme::default(), spec),
    };
    debug!("{:?} : {:?}", scheme, socket_address);

    Ok(EndPoint {
        socket: socket_address.parse().unwrap(),
        scheme,
        udp_cast_type: None
    })
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