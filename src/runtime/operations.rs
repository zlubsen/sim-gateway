use std::str::from_utf8;
use std::net::{IpAddr, SocketAddr};
use bytes::{Bytes, BytesMut};

type PacketMetaData<'a> = &'a(usize, SocketAddr);

pub trait Filter {
    fn filter(&self, buf : &Bytes, meta : PacketMetaData) -> bool;
}

pub trait Transformer {
    fn transform(&self, buf : BytesMut, meta : PacketMetaData) -> Result<Bytes,()>;
}

enum DefaultFilter {
    MinimumSize(usize),
    MaximumSize(usize),
    BlockHost(IpAddr),
    AllowHost(IpAddr),
}

impl Filter for DefaultFilter {
    fn filter(&self, buf : &Bytes, meta: PacketMetaData) -> bool {
        let (bytes_received, from_addr) = meta;
        return match self {
            DefaultFilter::MinimumSize(min_size) => { bytes_received >= min_size }
            DefaultFilter::MaximumSize(max_size) => { bytes_received <= max_size }
            DefaultFilter::BlockHost(block_ipaddr) => { from_addr.ip() != *block_ipaddr }
            DefaultFilter::AllowHost(allow_ipaddr) => { from_addr.ip() == *allow_ipaddr }
        }
    }
}

enum DefaultTransformer {
    CapitalizeUTF8(),
    RewriteSender(IpAddr),
}

impl Transformer for DefaultTransformer {
    fn transform(&self, buf: BytesMut, meta: PacketMetaData) -> Result<Bytes,()> {
        let (bytes_received, from_addr) = meta;
        return match self {
            DefaultTransformer::CapitalizeUTF8() => {
                let payload = from_utf8(&buf[..*bytes_received]);
                match payload {
                    Ok(text) => Ok(Bytes::from(text.to_uppercase())),
                    Err(err) => Err(()), // TODO proper error types
                }
            }
            DefaultTransformer::RewriteSender(_new_sender_ip) => { Ok(buf.freeze()) }
        }
    }
}