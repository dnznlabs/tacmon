//! What the monitors of a provider have in common: the readers every one of them needs, held to the same bounds, and
//! the few types those readers speak. Nothing here knows which provider is watched or what its amounts are amounts of.

pub mod chain;
pub mod feed;
pub mod http;
pub mod local;
pub mod model;
pub mod pool;
pub mod ports;
pub mod wire;
