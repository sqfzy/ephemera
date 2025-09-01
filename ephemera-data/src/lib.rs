pub mod data;
pub mod error;
pub mod stream;

pub use data::*;

pub type DataResult<T> = std::result::Result<T, error::DataError>;
pub type TimestampMs = u64;
pub type Symbol = bytestring::ByteString;
pub type IntervalSc = u64;
