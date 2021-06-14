use strum::EnumString;
use std::net::Ipv4Addr;
use std::sync::{Arc, RwLock};
use crate::model::arguments::Arguments;

#[derive(Clone)]
pub struct Config {
    pub mode : Mode,
    pub routes : Vec<Route>,
}

impl Config {
    pub fn current() -> Arc<Config> {
        CURRENT_CONFIG.with(|c| c.read().unwrap().clone())
    }
    pub fn make_current(self) {
        CURRENT_CONFIG.with(|c| *c.write().unwrap() = Arc::new(self))
    }
}

impl Default for Config {
    fn default() -> Self { Config {
        mode : Mode::Interactive,
        routes : Vec::new()
    } }
}

#[derive(Copy, Clone, Debug, PartialEq, EnumString)]
pub enum Mode {
    Interactive,
    Headless,
}

impl Default for Mode {
    fn default() -> Self { Mode::Interactive }
}

#[derive(Clone)]
pub struct Route {
    pub in_point : EndPoint,
    pub out_point : EndPoint,
    pub buffer_size : usize,
    pub flow_mode : FlowMode,
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

#[derive(Clone, Debug)]
pub struct EndPoint {
    pub ip : Ipv4Addr,
    pub port : u16,
    pub protocol : ProtocolType,
}

#[derive(Clone, Debug)]
pub enum ProtocolType {
    UDP(UdpCastType),
    TCP,
}

#[derive(Clone, Debug, PartialEq)]
pub enum UdpCastType {
    Unicast,
    Broadcast,
    Multicast,
}

#[derive(Clone, Debug)]
pub enum FlowMode {
    UniDirectional,
    BiDirectional,
}

thread_local! {
    static CURRENT_CONFIG: RwLock<Arc<Config>> = RwLock::new(Default::default());
}

pub fn merge(file_args: &Config, cli_args : &Config) -> Config {
    unimplemented!( )
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
