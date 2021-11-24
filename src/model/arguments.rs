use serde_derive::Deserialize;

#[derive(Debug, Deserialize)]
pub struct Arguments {
    pub metadata : MetaData,
    pub mode : Option<String>,
    pub routes : Vec<RouteSpec>,
}

#[derive(Debug, Deserialize)]
pub struct MetaData {
    pub name : String,
    pub author : String,
    pub version : String,
}

#[derive(Debug, Deserialize)]
pub struct RouteSpec {
    pub name: String,
    pub in_point: EndPointSpec,
    pub out_point: EndPointSpec,
    pub buffer_size: Option<usize>,
    pub max_connections: Option<usize>,
    pub flow_mode : Option<String>,
    pub block_host : Option<bool>,
    pub enabled: Option<bool>,
    pub filters: Option<Vec<String>>,
}

#[derive(Debug, Deserialize)]
pub struct EndPointSpec {
    pub uri: String,
    pub interface: String,
    pub kind: Option<String>,
    pub ttl: Option<u32>,
}