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
    pub in_point: String,
    pub out_point: String,
    pub buffer_size: Option<u64>,
    pub flow_mode : Option<String>,
    pub enabled: Option<bool>,
}