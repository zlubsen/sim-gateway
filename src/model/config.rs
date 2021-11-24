use std::net::{IpAddr, SocketAddrV4};
use std::sync::{Arc, RwLock};
use std::str::FromStr;
use std::convert::TryFrom;
use std::fmt::{Display, Formatter};
use std::fmt;
use std::error::Error;

use strum::{EnumString, IntoStaticStr};
use log::{trace, error};
use lazy_static::lazy_static;

use crate::model::arguments::{Arguments, RouteSpec, EndPointSpec};
use crate::model::constants::*;

use crate::model::filters::{filter_from_str, Filters};
use crate::model::transformers::{transformer_from_str, Transformers};

lazy_static! {
    static ref CURRENT_CONFIG: RwLock<Arc<Config>> = RwLock::new(Default::default());
}

type FilterList = Vec<Arc<Filters>>;
type TransformerList = Vec<Arc<Transformers>>;

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

#[derive(Debug)]
pub struct Config {
    pub mode : Mode,
    pub routes : Vec<Route>,
}

impl Config {
    pub fn current() -> Arc<Config> {
        CURRENT_CONFIG.read().unwrap().clone()
    }
    pub fn make_new_current(self) {
        *CURRENT_CONFIG.write().unwrap() = Arc::new(self)
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
    pub max_connections: usize,
    pub flow_mode : FlowMode,
    pub block_host : bool,
    pub enabled : bool,
    pub filters : FilterList,
    pub transformers : TransformerList,
}

impl Route {
    pub fn new(name : String, in_point : EndPoint, out_point : EndPoint, buffer_size : usize,
               max_connections : usize, flow_mode : FlowMode, block_host : bool, enabled : bool,
               filters : FilterList, transformers : TransformerList) -> Route {
        return Route {
            name,
            in_point,
            out_point,
            buffer_size,
            max_connections,
            flow_mode,
            block_host,
            enabled,
            filters,
            transformers,
        }
    }
}

impl TryFrom<&RouteSpec> for Route {
    type Error = ConfigError;

    fn try_from(spec: &RouteSpec) -> Result<Self, Self::Error> {
        let name = String::from(spec.name.as_str()); // TODO name cannot be empty string
        let in_point = EndPoint::try_from(&spec.in_point)?;
        let out_point = EndPoint::try_from(&spec.out_point)?;
        let buffer_size = spec.buffer_size.unwrap_or(DEFAULT_BUFFER_SIZE_BYTES);
        let max_connections = spec.max_connections.unwrap_or(DEFAULT_MAX_CONNECTIONS);

        let flow_mode = if let Some(val) = &spec.flow_mode {
            match FlowMode::from_str(val.as_str()) {
                Ok(mode) => mode,
                Err(err) => { return Err(ConfigError::RouteError(format!("{} - {}", name, err))); }
            }
        } else { FlowMode::default() };
        let block_host = spec.block_host.unwrap_or(DEFAULT_BLOCK_HOST);
        let enabled = spec.enabled.unwrap_or(DEFAULT_ENABLED);

        let filters = if let Some(filter_names) = &spec.filters {
            filter_names.iter().map(|name| filter_from_str(name.as_str()))
                .map(|f| f.unwrap())
                // .map(|f| {let x : Box<dyn Filter> = Box::new(f); x})
                .map(|f| Arc::new(f))
                .collect()
        } else { Vec::new() };

        let transformers = if let Some(transformer_names) = &spec.filters {
            transformer_names.iter().map(|name| transformer_from_str(name.as_str()))
                .map(|t| t.unwrap())
                // .map(|t| {let x : Box<dyn Transformer> = Box::new(t); x})
                .map(|t| Arc::new(t))
                .collect()
        } else { Vec::new() };

        Ok(Route{
            name,
            in_point,
            out_point,
            buffer_size,
            max_connections,
            flow_mode,
            block_host,
            enabled,
            filters,
            transformers,
        })
    }
}

#[derive(Clone, Debug)]
pub struct EndPoint {
    pub socket : SocketAddrV4,
    pub scheme : Scheme,
    pub socket_type : SocketType,
    pub ttl : u32,
    pub interface : IpAddr,
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
        let ttl = spec.ttl.unwrap_or(DEFAULT_TTL);
        let interface = spec.interface.parse().expect("Invalid interface provided in config file.");

        Ok(EndPoint {
            socket: socket_address,
            scheme,
            socket_type,
            ttl,
            interface,
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

#[derive(Copy, Clone, Debug, EnumString, PartialEq)]
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
