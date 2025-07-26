use crate::http::DynamicIterator;
use std::collections::HashMap;
use std::rc::Rc;
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use ratelimit::{Alignment, Ratelimiter};
use crate::gate::{CONNECT_LIMITER, HTTP_LIMITER};
use crate::gate::auxiliar::error::ExchangeError;
use crate::gate::auxiliar::utils::GateExchangeUtils;

macro_rules! from_headers {
  ($headers:expr) => {
    &mut $headers
      .iter()
      .map(|(k, v)| (k as &dyn AsRef<str>, v as &dyn AsRef<str>))
    as DynamicIterator<(&dyn AsRef<str>, &dyn AsRef<str>)>
  };
}

pub async fn sync_time(utils: Rc<GateExchangeUtils>) -> Result<(), ExchangeError> {
  let url = "https://api.gateio.ws/api/v4/spot/time";
  let headers: HashMap<String, String> = HashMap::new();

  let response = utils
    .http_client
    .request(
      "GET".to_string(),
      url.to_string(),
      from_headers!(headers),
      None,
    ) // ajuste conforme seu client
    .await
    .map_err(|e| ExchangeError::ApiError(format!("Sync time error: {e}")))?
    .body()
    .await
    .map_err(|e| ExchangeError::ApiError(format!("Invalid body: {e}")))?;

  let response = std::str::from_utf8(&response)
    .map_err(|e| ExchangeError::ApiError(format!("Invalid response {e:?}")))?;

  let json: serde_json::Value =
    serde_json::from_str(response).map_err(ExchangeError::JsonError)?;

  if let Some(server_time) = json.get("server_time").and_then(|v| v.as_i64()) {
    let local_time = SystemTime::now()
      .duration_since(UNIX_EPOCH)
      .unwrap()
      .as_millis() as i64;

    let connnect_limiter = Ratelimiter::builder(274, Duration::from_secs(300))
      .max_tokens(274)
      .initial_available(274)
      .alignment(Alignment::Minute)
      .sync_time(server_time as u64)
      .build()
      .unwrap();

    let _ = CONNECT_LIMITER.set(Arc::new(connnect_limiter));

    let http_limiter = Ratelimiter::builder(200, Duration::from_millis(11500))
      .max_tokens(200)
      .initial_available(200 - 1)
      .alignment(Alignment::Second)
      .sync_time(server_time as u64)
      .build()
      .unwrap();

    let _ = HTTP_LIMITER.set(Arc::new(http_limiter));

    Ok(())
  } else {
    Err(ExchangeError::ApiError(
      "Invalid server time response".into(),
    ))
  }
}