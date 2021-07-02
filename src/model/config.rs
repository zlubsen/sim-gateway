use strum::EnumString;
use std::net::{Ipv4Addr, SocketAddr};
use std::sync::{Arc, RwLock};
use crate::model::arguments::Arguments;

#[derive(Debug, Clone)]
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

impl From<&str> for Mode {
    fn from(val: &str) -> Self {
        return match val {
            "interactive" => Self::Interactive,
            "headless" => Self::Headless,
            _ => Self::default(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct Route {
    pub name : String,
    pub in_point : EndPoint,
    pub out_point : EndPoint,
    pub buffer_size : usize,
    pub flow_mode : FlowMode,
    // filters_in : Vec<Filter>,
    // filters_out : Vec<Filter>,
    // transforms : Vec<Transform>
}

impl Route {
    pub fn new(name : String, in_point : EndPoint, out_point : EndPoint, buffer_size : usize, flow_mode : FlowMode) -> Route {
        return Route {
            name,
            in_point,
            out_point,
            buffer_size,
            flow_mode,
        }
    }
}

// #[derive(Clone, Debug)]
// pub enum EndPointType {
//     UdpListen {
//         socket: SocketAddr,
//     },
//     UdpSend {
//         socket : SocketAddr,
//         cast_type : UdpCastType,
//     },
//     TcpServer {
//         socket : SocketAddr,
//     },
//     TcpClient {
//         socket : SocketAddr,
//     },
// }

#[derive(Clone, Debug)]
pub struct EndPoint {
    pub socket : SocketAddr,
    pub scheme: Scheme,
    pub udp_cast_type : Option<UdpCastType>,
}

#[derive(Clone, Debug)]
pub enum Scheme {
    UDP,
    TCP,
}

impl Default for Scheme {
    fn default() -> Self {
        Scheme::UDP
    }
}

impl From<&str> for Scheme {
    fn from(val: &str) -> Self {
        return match val {
            "udp" => Self::UDP,
            "tcp" => Self::TCP,
            _ => Self::default(),
        }
    }
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
