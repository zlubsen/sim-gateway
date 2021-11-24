use std::net::SocketAddr;
use bytes::BytesMut;

pub mod arguments;
pub mod config;
pub mod constants;
pub mod filters;
pub mod transformers;

/// Wrapper type for the length of the passed buffer, and the SocketAddr the packet is received from
type PacketMetaData<'a> = &'a(usize, SocketAddr);

pub trait Filter : Send + Sync {
    fn filter(&self, buf : &BytesMut, meta : PacketMetaData) -> bool;
}

pub trait Transformer : Send + Sync{
    fn transform(&self, buf : &BytesMut, meta : PacketMetaData) -> Result<BytesMut,()>;
}
