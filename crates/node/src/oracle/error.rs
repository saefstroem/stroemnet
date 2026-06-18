use thiserror::Error;

#[derive(Error, Debug)]
pub enum OracleError {
    #[error("Reqwest error: {0}")]
    Reqwest(#[from] reqwest::Error),

    #[error("JSON parsing error: {0}")]
    JsonParse(#[from] serde_json::Error),

    #[error("Float parsing error: {0}")]
    FloatParse(#[from] std::num::ParseFloatError),

    #[error("Request timeout after {0} seconds")]
    Timeout(u64),

    #[error("No price data available for {0}")]
    NoPriceData(String),

    #[error("Failed to fetch any prices for channel: {0}")]
    AllSourcesFailed(String),

    #[error("All {0} retry attempts failed: {1}")]
    RetryExhausted(usize, String),

    #[error("Missing field in API response: {0}")]
    MissingField(String),

    #[error("Std io error: {0}")]
    Io(#[from] std::io::Error),

    #[error("Parse int error: {0}")]
    ParseInt(#[from] std::num::ParseIntError),

    #[error("System time error: {0}")]
    SystemTime(#[from] std::time::SystemTimeError),
}
