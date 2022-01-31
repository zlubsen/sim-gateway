use std::slice::Iter;
use std::str::from_utf8;
use std::sync::Arc;

use bytes::{Bytes, BytesMut};

use crate::model::{PacketMetaData, Transformer};

/// To add a transformer:
/// - create a struct for the Transformer
/// - implement trait Transformer for the struct
/// - add it to the Transformers enum
/// - add a line to the transformer_from_str function to create the type

#[derive(Debug, Clone)]
pub enum Transformers {
    None(None),
    CapitalizeUTF8(CapitalizeUTF8),
}

impl Transformer for Transformers {
    fn transform(&self, buf: &BytesMut, meta: PacketMetaData) -> Result<BytesMut, ()> {
        return match self {
            Transformers::None(none) => { none.transform(buf, meta) }
            Transformers::CapitalizeUTF8(cap) => { cap.transform(buf, meta) }
        }
    }
}

#[derive(Debug, Clone)]
pub struct None {}

impl Transformer for None {
    fn transform(&self, buf: &BytesMut, _meta: PacketMetaData) -> Result<BytesMut,()> {
        Ok(buf.clone())
    }
}

#[derive(Debug, Clone)]
pub struct CapitalizeUTF8 {}

impl Transformer for CapitalizeUTF8 {
    fn transform(&self, buf: &BytesMut, meta: PacketMetaData) -> Result<BytesMut,()> {
        let (bytes_received, _from_addr) = meta;
        let payload = from_utf8(&buf[..*bytes_received]);
        return match payload {
            Ok(text) => {
                let out = text.to_uppercase();
                Ok(BytesMut::from(out.as_str()))
            },
            Err(_err) => Err(()), // TODO proper error types
        }
    }
}

pub fn transformer_from_str(s: &str) -> Result<Transformers, ()> {
    let sections : Vec<&str> = s.split(':').collect();
    let transformer_name = sections[0];
    let _argument_value = if sections.len() > 1 { sections[1] } else { "" };
    let transformer = match transformer_name {
        "None" => { Transformers::None(None {}) }
        "CapitalizeUTF8" => { Transformers::CapitalizeUTF8(CapitalizeUTF8 {}) }
        _ => { Transformers::None(None {}) }
    };
    Ok(transformer)
}

pub fn apply_transformers(buf : &BytesMut, transformers: Iter<Arc<Transformers>>, meta : &PacketMetaData) -> Bytes {
    return if transformers.len() > 0 {
        let input_buf = buf;
        let mut output_buf = BytesMut::new();
        for t in transformers {
            output_buf = t.transform(input_buf, meta).unwrap();
        }
        output_buf.freeze()
    } else {
        Bytes::copy_from_slice(&buf[..meta.0])
    }
}