#![deny(clippy::all)]
#![deny(unsafe_code)]
#![allow(clippy::needless_lifetimes)]

mod rope;
mod text;

pub use rope::*;
pub use text::*;
