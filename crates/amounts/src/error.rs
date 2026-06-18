use alloy_primitives::U256;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum AmountError {
    #[error("Invalid price data: {0}")]
    InvalidPriceData(f64),

    #[error("Arithmetic overflow: {0:?}")]
    ArithmeticOverflow(U256),

    #[error("Division by zero: {0:?}")]
    DivisionByZero(U256),

    #[error("Amount overflow: {0}")]
    AmountOverflow(U256),

    #[error("Amount underflow: {0}")]
    AmountUnderflow(U256),
}
