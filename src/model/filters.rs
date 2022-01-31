use std::net::IpAddr;
use std::str::FromStr;
use std::slice::Iter;
use std::sync::Arc;

use bytes::BytesMut;

use crate::model::{Filter, PacketMetaData};

/// To add a filter:
/// - create a struct for the Filter
/// - implement trait Filter for the struct
/// - add it to the Filters enum
/// - add a line to the filter_from_str function to create the type

#[derive(Debug, Clone)]
pub enum Filters {
    PassAll(PassAll),
    BlockAll(BlockAll),
    MinimumSize(MinimumSize),
    MaximumSize(MaximumSize),
    BlockHost(BlockHost),
    AllowHost(AllowHost),
}

impl Filter for Filters {
    fn filter(&self, buf: &BytesMut, meta: PacketMetaData) -> bool {
        return match self {
            Filters::PassAll(f) => { f.filter(buf, meta) }
            Filters::BlockAll(f) => { f.filter(buf, meta) }
            Filters::MinimumSize(f) => { f.filter(buf, meta) }
            Filters::MaximumSize(f) => { f.filter(buf, meta) }
            Filters::BlockHost(f) => { f.filter(buf, meta) }
            Filters::AllowHost(f) => { f.filter(buf, meta) }
        }
    }
}

#[derive(Debug, Clone)]
pub struct PassAll {}

impl Filter for PassAll {
    fn filter(&self, _buf: &BytesMut, _meta: PacketMetaData) -> bool {
        true
    }
}

#[derive(Debug, Clone)]
pub struct BlockAll {}

impl Filter for BlockAll {
    fn filter(&self, _buf: &BytesMut, _meta: PacketMetaData) -> bool {
        false
    }
}

#[derive(Debug, Clone)]
pub struct MinimumSize {
    min_size : usize,
}

impl Filter for MinimumSize {
    fn filter(&self, _buf: &BytesMut, meta: PacketMetaData) -> bool {
        let (bytes_received, _from_addr) = meta;
        bytes_received >= &self.min_size
    }
}

#[derive(Debug, Clone)]
pub struct MaximumSize {
    max_size : usize,
}

impl Filter for MaximumSize {
    fn filter(&self, _buf: &BytesMut, meta: PacketMetaData) -> bool {
        let (bytes_received, _from_addr) = meta;
        bytes_received <= &self.max_size
    }
}

#[derive(Debug, Clone)]
pub struct BlockHost {
    address : IpAddr,
}

impl Filter for BlockHost {
    fn filter(&self, _buf: &BytesMut, meta: PacketMetaData) -> bool {
        let (_bytes_received, from_addr) = meta;
        from_addr.ip() != self.address
    }
}

#[derive(Debug, Clone)]
pub struct AllowHost {
    address : IpAddr,
}

impl Filter for AllowHost {
    fn filter(&self, _buf: &BytesMut, meta: PacketMetaData) -> bool {
        let (_bytes_received, from_addr) = meta;
        from_addr.ip() == self.address
    }
}

pub fn filter_from_str(s: &str) -> Result<Filters, ()> {
    let sections : Vec<&str> = s.split(':').collect();
    let filter_name = sections[0];
    let argument_value = if sections.len() > 1 { sections[1] } else { "" };
    let filter = match filter_name {
        "PassAll" => { Filters::PassAll(PassAll{}) }
        "BlockAll" => { Filters::BlockAll(BlockAll{}) }
        "MinimumSize" => { Filters::MinimumSize(MinimumSize{ min_size : usize::from_str(argument_value).unwrap() }) }
        "MaximumSize" => { Filters::MaximumSize(MaximumSize{ max_size : usize::from_str(argument_value).unwrap() }) }
        "BlockHost" => { Filters::BlockHost(BlockHost { address : IpAddr::from_str(argument_value).unwrap() }) }
        "AllowHost" => { Filters::AllowHost(AllowHost { address : IpAddr::from_str(argument_value).unwrap() }) }
        _ => { Filters::PassAll(PassAll{}) }
    };
    Ok(filter)
}

pub fn passes_filters(buf : &BytesMut, filters: Iter<Arc<Filters>>, meta : &PacketMetaData) -> bool {
    if filters.len() == 0 { return true; }

    let passes = filters.map(|f| {
        f.filter(buf, meta)})
        .reduce(|a,b| a & b );
    passes.unwrap_or(false)
}