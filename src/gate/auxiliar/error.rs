use thiserror::Error;

#[derive(Error, Debug)]
pub enum ExchangeError {
  #[error("Bad request: {0}")]
  BadRequest(String),
  #[error("Invalid nonce: {0}")]
  InvalidNonce(String),
  #[error("Bad symbol: {0}")]
  BadSymbol(String),
  #[error("Invalid order: {0}")]
  InvalidOrder(String),
  #[error("Invalid address: {0}")]
  InvalidAddress(String),
  #[error("Insufficient funds: {0}")]
  InsufficientFunds(String),
  #[error("Permission denied: {0}")]
  PermissionDenied(String),
  #[error("Authentication error: {0}")]
  AuthenticationError(String),
  #[error("Account suspended: {0}")]
  AccountSuspended(String),
  #[error("On maintenance: {0}")]
  OnMaintenance(String),
  #[error("Not supported: {0}")]
  NotSupported(String),
  #[error("Network error: {0}")]
  NetworkError(#[from] Box<dyn std::error::Error + Send + Sync>), // Catches generic errors from GlommioHttpClient
  #[error("JSON parsing error: {0}")]
  JsonError(#[from] serde_json::Error),
  #[error("API error: {0}")]
  ApiError(String),
  #[error("Missing credentials")]
  MissingCredentials,
}