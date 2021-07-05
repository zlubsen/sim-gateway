pub mod arguments;
pub mod config;

// pub use config::*;
// pub use arguments::*;

use arguments::*;
use config::*;
use log::debug;
use std::string::ParseError;
use std::convert::TryInto;

// trait Filter {
//     fn new() -> Self;
//     fn apply(&self) -> bool;
// }
//
// trait Transform {
//     fn new() -> Self;
//     fn apply(&self) -> &self;
// }