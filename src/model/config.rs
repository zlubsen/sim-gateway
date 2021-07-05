use strum::{EnumString, IntoStaticStr};
use std::net::{SocketAddr};
use std::sync::{Arc, RwLock};
use crate::model::arguments::{Arguments, RouteSpec};
use std::str::FromStr;
use std::convert::TryFrom;

use log::{error, warn, info, debug, trace};

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

impl TryFrom<&Arguments> for Config {
    type Error = &'static str;

    fn try_from(args: &Arguments) -> Result<Self, Self::Error> {
        let mode = match &args.mode {
            Some(mode_arg) => Mode::from_str(mode_arg).unwrap(),
            None => Mode::default(),
        };

        let parsed_routes : Vec<Result<Route, Self::Error>> = args.routes.iter()
            .map(|route_spec| { Route::try_from(route_spec) } ).collect();

        parsed_routes.iter().filter(|&res| res.is_err()).for_each(|err|error!("{}", err.unwrap_err()));

        let routes = parsed_routes.iter()
            .filter(|res|res.is_ok())
            .map(|ok|ok.unwrap())
            .collect();

        Ok(Config {
            mode,
            routes,
        })
    }
}

#[derive(Copy, Clone, Debug, PartialEq, EnumString)]
pub enum Mode {
    Interactive,
    Headless,
}

impl Default for Mode {
    fn default() -> Self { Mode::Interactive }
}

// impl FromStr for Mode {
//     type Err = &'static str;
//
//     fn from_str(value: &str) -> Result<Self, Self::Err> {
//         return match value {
//             "interactive" => Ok(Self::Interactive),
//             "headless" => Ok(Self::Headless),
//             _ => Self::Err("Error parsing mode '{}'", value),
//         }
//     }
// }

// // TODO replace with strum?
// impl From<&str> for Mode {
//     fn from(val: &str) -> Self {
//         return match val {
//             "interactive" => Self::Interactive,
//             "headless" => Self::Headless,
//             _ => Self::default(),
//         }
//     }
// }

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

impl TryFrom<&RouteSpec> for Route {
    type Error = &'static str;

    fn try_from(spec: &RouteSpec) -> Result<Self, Self::Error> {
        let name = String::from(spec.name.as_str()); // TODO name cannot be empty string
        let in_point = EndPoint::try_from(spec.in_point.as_str())?;
        let out_point = EndPoint::try_from(spec.out_point.as_str())?;
        let buffer_size = spec.buffer_size.unwrap_or(32768) as usize;

        let flow_mode = if let Some(val) = &spec.flow_mode {
            match FlowMode::from_str(val.as_str()) {
                Ok(mode) => mode,
                Err(err) => { return Err(format!("{}", err).as_str()); }
            }
        } else { FlowMode::default() };

        Ok(Route{
            name,
            in_point,
            out_point,
            buffer_size,
            flow_mode,
        })
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

impl TryFrom<&str> for EndPoint {
    type Error = &'static str;

    fn try_from(value: &str) -> Result<Self, Self::Error> {
        let (scheme_val, socket_address_val) : (&str, &str) = match value.split_once("://") {
            Some((val, tail)) => (val, tail),
            None => (Scheme::default().into(), value),
        };
        let scheme : Scheme = match Scheme::from_str(scheme_val) {
            Ok(scheme) => scheme,
            Err(err) => return Err(format!("Error parsing endpoint {}", value).as_str()),
        };
        // TODO input validation for socket_address_val
        let socket_address = socket_address_val.parse().unwrap();
        debug!("{:?} : {:?}", scheme, socket_address);

        Ok(EndPoint {
            socket: socket_address,
            scheme,
            udp_cast_type: None
        })
    }
}

#[derive(Clone, Debug, EnumString, IntoStaticStr)]
pub enum Scheme {
    #[strum(serialize = "udp", to_string="udp")]
    UDP,
    #[strum(serialize = "tcp", to_string="tcp")]
    TCP,
}

impl Default for Scheme {
    fn default() -> Self {
        Scheme::UDP
    }
}

// // TODO replace with strum?
// impl From<&str> for Scheme {
//     fn from(val: &str) -> Self {
//         return match val {
//             "udp" => Self::UDP,
//             "tcp" => Self::TCP,
//             _ => Self::default(),
//         }
//     }
// }

#[derive(Clone, Debug, PartialEq)]
pub enum UdpCastType {
    Unicast,
    Broadcast,
    Multicast,
}

#[derive(Clone, Debug, EnumString)]
pub enum FlowMode {
    UniDirectional,
    BiDirectional,
}

impl Default for FlowMode {
    fn default() -> Self {
        FlowMode::UniDirectional
    }
}

thread_local! {
    static CURRENT_CONFIG: RwLock<Arc<Config>> = RwLock::new(Default::default());
}

pub fn merge(file_args: &Config, cli_args : &Config) -> Config {
    unimplemented!( )
}
