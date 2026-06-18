use super::error::HandlerError;

pub type Result<T> = std::result::Result<T, HandlerError>;
