#![warn(unreachable_pub)]
#![allow(clippy::result_large_err)]

mod buffer;
mod chains;
pub mod error;
mod sink;
mod store;
mod types;

#[cfg(not(target_arch = "wasm32"))]
pub(crate) use buffer::TaskFut;
pub(crate) use buffer::{ChainDataBuffer, UtxoScriptDetector};
pub use chains::record::AttemptState;
pub use chains::settlement::{Gauge, Metric, NoopMetrics, SettlementMetrics};
pub use error::{DataError, Result};
pub use sink::ChainDataSink;
pub use store::{CursorStore, PersistedSwap, SwapStore};
pub(crate) use types::BufFut;
pub use types::{MaybeSend, ProposalVerification, ScriptAnnouncement, UtxoScript};
