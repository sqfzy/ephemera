#![feature(try_blocks)]
#![feature(gen_blocks)]
#![feature(async_fn_traits)]

pub mod config;
pub mod data;
pub mod order;
pub mod source;
pub mod xdp;

pub type Timestamp = u64;
pub type Symbol = bytestring::ByteString;

// TODO: 也许可以有raw feature，让用户可以拿到raw api
