use strum::{EnumString, IntoStaticStr};
use std::net::{SocketAddr};
use std::sync::{Arc, RwLock};
use crate::model::arguments::{Arguments, RouteSpec, EndPointSpec};
use std::str::FromStr;
use std::convert::TryFrom;
use std::fmt::{Display, Formatter};
use std::fmt;
use std::error::Error;

use log::{error, warn, info, debug, trace};

thread_local! {
    static CURRENT_CONFIG: RwLock<Arc<Config>> = RwLock::new(Default::default());
}

#[derive(Debug)]
pub enum ConfigError {
    ModeError(String),
    RouteError(String),
    EndPointError(String),
    SchemeError(String),
}

impl Error for ConfigError {
}

impl Display for ConfigError {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match self {
            ConfigError::ModeError(s) => write!(f, "Error parsing mode: {}", s),
            ConfigError::RouteError(s) => write!(f, "Error parsing route: {}", s),
            ConfigError::EndPointError(s) => write!(f, "Error parsing endpoint: {}", s),
            ConfigError::SchemeError(s) => write!(f, "Error parsing mode: {}", s),
        }
    }
}

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
    type Error = ConfigError;

    fn try_from(args: &Arguments) -> Result<Self, Self::Error> {
        let mode = match &args.mode {
            Some(mode_arg) => Mode::from_str(mode_arg).unwrap(),
            None => Mode::default(),
        };

        let parsed_routes : Vec<Result<Route, Self::Error>> = args.routes.iter()
            .map(|route_spec| { Route::try_from(route_spec) } ).collect();

        parsed_routes.iter().filter(|&res| res.is_err()).for_each(|err|error!("{}", err.as_ref().unwrap_err()));

        let routes = parsed_routes.iter()
            .filter(|res|res.is_ok())
            .map(|ok|ok.as_ref().unwrap().to_owned())
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
    fn default() -> Self { Mode::Headless }
}

#[derive(Debug, Clone)]
pub struct Route {
    pub name : String,
    pub in_point : EndPoint,
    pub out_point : EndPoint,
    pub buffer_size : usize,
    pub flow_mode : FlowMode,
    pub enabled : bool,
}

impl Route {
    pub fn new(name : String, in_point : EndPoint, out_point : EndPoint, buffer_size : usize, flow_mode : FlowMode, enabled : bool) -> Route {
        return Route {
            name,
            in_point,
            out_point,
            buffer_size,
            flow_mode,
            enabled,
        }
    }
}

impl TryFrom<&RouteSpec> for Route {
    type Error = ConfigError;

    fn try_from(spec: &RouteSpec) -> Result<Self, Self::Error> {
        let name = String::from(spec.name.as_str()); // TODO name cannot be empty string
        let in_point = EndPoint::try_from(&spec.in_point)?;
        // let in_point = if let Some(cast_type) = &spec.in_point_cast_type {
        //     in_point.add_cast_type(cast_type)?
        // } else { in_point };
        let out_point = EndPoint::try_from(&spec.out_point)?;
        // let out_point = if let Some(cast_type) = &spec.out_point_cast_type {
        //     out_point.add_cast_type(cast_type)?
        // } else { out_point };
        let buffer_size = spec.buffer_size.unwrap_or(32768) as usize;

        let flow_mode = if let Some(val) = &spec.flow_mode {
            match FlowMode::from_str(val.as_str()) {
                Ok(mode) => mode,
                Err(err) => { return Err(ConfigError::RouteError(format!("{} - {}", name, err))); }
            }
        } else { FlowMode::default() };
        let enabled = if let Some(value) = spec.enabled {
            value
        } else { true };

        Ok(Route{
            name,
            in_point,
            out_point,
            buffer_size,
            flow_mode,
            enabled
        })
    }
}

const DEFAULT_TTL : u8 = 64; // default for unix and mac

#[derive(Clone, Debug)]
pub struct EndPoint {
    pub socket : SocketAddr,
    pub scheme: Scheme,
    pub socket_type: SocketType,
    pub ttl: u32,
}

impl TryFrom<&EndPointSpec> for EndPoint {
    type Error = ConfigError;

    fn try_from(spec: &EndPointSpec) -> Result<Self, Self::Error> {
        let (scheme_val, socket_address_val): (&str, &str) = match spec.uri.split_once("://") {
            Some((val, tail)) => (val, tail),
            None => (Scheme::default().into(), &*spec.uri),
        };
        let scheme: Scheme = match Scheme::from_str(scheme_val) {
            Ok(scheme) => scheme,
            Err(err) => return Err(ConfigError::EndPointError(format!("{} for {}", err, spec.uri))),
        };
        let socket_address = match socket_address_val.parse() {
            Ok(val) => val,
            Err(err) => return Err(ConfigError::EndPointError(format!("{} for {}", err, socket_address_val)))
        };
        let socket_type = match scheme {
            Scheme::UDP => {
                if let Some(kind) = &spec.kind {
                    match UdpSocketType::from_str(kind) {
                        Ok(val) => SocketType::UdpSocketType(val),
                        Err(err) => return Err(ConfigError::EndPointError(format!("{} for {}", err, kind)))
                    }
                } else { SocketType::UdpSocketType(UdpSocketType::default()) }
            },
            Scheme::TCP => {
                if let Some(kind) = &spec.kind {
                    match TcpSocketType::from_str(kind) {
                        Ok(val) => SocketType::TcpSocketType(val),
                        Err(err) => return Err(ConfigError::EndPointError(format!("{} for {}", err, kind)))
                    }
                } else { SocketType::TcpSocketType(TcpSocketType::default()) }
            }
        };
        let ttl = if let Some(ttl_val) = spec.ttl {
            ttl_val
        } else { DEFAULT_TTL };

        Ok(EndPoint {
            socket: socket_address,
            scheme,
            socket_type,
            ttl,
        })
    }
}

#[derive(Clone, Debug, EnumString, IntoStaticStr)]
pub enum Scheme {
    #[strum(serialize = "udp")]
    UDP,
    #[strum(serialize = "tcp")]
    TCP,
}

impl Default for Scheme {
    fn default() -> Self {
        Scheme::UDP
    }
}

#[derive(Clone, Debug, PartialEq, EnumString)]
pub enum SocketType {
    UdpSocketType(UdpSocketType),
    TcpSocketType(TcpSocketType),
}

#[derive(Clone, Debug, PartialEq, EnumString)]
pub enum UdpSocketType {
    Unicast,
    Broadcast,
    Multicast,
}

impl Default for UdpSocketType {
    fn default() -> Self {
        UdpSocketType::Unicast
    }
}

#[derive(Clone, Debug, PartialEq, EnumString)]
pub enum TcpSocketType {
    Server,
    Client,
}

impl Default for TcpSocketType {
    fn default() -> Self {
        TcpSocketType::Server
    }
}

#[derive(Clone, Debug, EnumString)]
pub enum FlowMode {
    #[strum(serialize = "unidirectional")]
    UniDirectional,
    #[strum(serialize = "bidirectional")]
    BiDirectional,
}

impl Default for FlowMode {
    fn default() -> Self {
        FlowMode::UniDirectional
    }
}

pub fn merge_configs(file_args: Option<Config>, cli_args : Option<Config>) -> Config {
    if file_args.is_some() && cli_args.is_some() {
        Config {
            mode : cli_args.unwrap().mode,
            routes : file_args.unwrap().routes,
        }
    } else if file_args.is_some() && cli_args.is_none() {
        file_args.unwrap()
    } else if file_args.is_none() && cli_args.is_some() {
        cli_args.unwrap()
    } else { Config::default() }
}
