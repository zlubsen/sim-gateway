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
    // pub in_point_type : Option<String>,
    pub out_point: EndPointSpec,
    // pub out_point_type : Option<String>,
    pub buffer_size: Option<u64>,
    pub flow_mode : Option<String>,
    pub enabled: Option<bool>,
}

#[derive(Debug, Deserialize)]
pub struct EndPointSpec {
    pub uri: String,
    pub kind: Option<String>,
    pub ttl: Option<u32>,
}