//! Amounts crate for handling token amounts, conversions, and price storage.

/// A submodule for computing amounts out given some parameters
pub mod amount_out;
mod error;
mod result;
mod storage;

// Re-export the main types and results for external use.
pub use amount_out::Amounts;
pub use error::AmountError;
pub use storage::PriceStorage;
