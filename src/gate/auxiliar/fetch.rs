use crate::http::DynamicIterator;
use std::collections::HashMap;
use std::rc::Rc;
use futures::join;
use crate::from_headers;
use crate::gate::{ASSETS, HTTP_LIMITER};
use crate::gate::auxiliar::asset::{Asset, Assets, MarketType};
use crate::gate::auxiliar::error::ExchangeError;
use crate::gate::auxiliar::utils::GateExchangeUtils;

async fn fetch_assets_by(market: MarketType, utils: Rc<GateExchangeUtils>) -> Result<Vec<Asset>, ExchangeError> {
  let headers: HashMap<String, String> = HashMap::new();

  let url = if let MarketType::Spot = market {
    "https://api.gateio.ws/api/v4/spot/currency_pairs"
  } else {
    "https://fx-api.gateio.ws/api/v4/futures/usdt/contracts"
  };

  let response = loop {
    match HTTP_LIMITER
      .get()
      .expect("Limiter not initialized")
      .try_wait()
    {
      Ok(()) => {
        break utils
          .http_client
          .request(
            "GET".to_string(),
            url.to_string(),
            from_headers!(headers),
            None,
          )
          .await
          .map_err(|e| ExchangeError::ApiError(format!("Erro ao buscar ativos: {e}")))?
          .body()
          .limit(100 * 1024 * 1024)
          .await
          .map_err(|e| ExchangeError::ApiError(format!("Corpo inválido: {e}")))?;
      }
      Err(duration) => {
        ntex::time::sleep(duration).await;
      }
    }
  };

  let response = std::str::from_utf8(&response)
    .map_err(|e| ExchangeError::ApiError(format!("Resposta inválida: {e:?}")))?;

  let json: serde_json::Value =
    serde_json::from_str(response).map_err(ExchangeError::JsonError)?;

  let mut assets = Vec::new();

  if let Some(symbols) = json.as_array() {
    for symbol in symbols {
      if let MarketType::Future = market {
        if symbol.get("status").and_then(|v| v.as_str()) != Some("trading") {
          continue;
        }
      } else if symbol.get("trade_status").and_then(|v| v.as_str()) != Some("tradable") {
        continue;
      }

      let name;

      if let MarketType::Future = market {
        name = symbol
          .get("name")
          .and_then(|v| v.as_str())
          .unwrap_or_default();
      } else {
        name = symbol
          .get("id")
          .and_then(|v| v.as_str())
          .unwrap_or_default();
      }

      let mut parts = name.splitn(2, '_');

      let base = parts.next().unwrap_or("");
      let quote = parts.next().unwrap_or("");

      let symbol_name;

      if let MarketType::Spot = market {
        symbol_name = format!("{base}/{quote}");
      } else {
        symbol_name = format!("{base}/{quote}:{quote}");
      }

      assets.push(Asset {
        symbol: symbol_name,
        base: base.to_string(),
        quote: quote.to_string(),
        market: market.clone(),
        exchange: "Gate".to_string(),
      });
    }
  }

  Ok(assets)
}

pub async fn fetch_assets(utils: Rc<GateExchangeUtils>) -> Result<Assets, ExchangeError> {
  let (spot, future) = join!(
      fetch_assets_by(MarketType::Spot, utils.clone()),
      fetch_assets_by(MarketType::Future, utils)
    );

  let (spot, future) = (spot?, future?);

  let spot = spot
    .into_iter()
    .map(|asset| (asset.symbol.clone(), asset))
    .collect();

  let future = future
    .into_iter()
    .map(|asset| (asset.symbol.clone(), asset))
    .collect();

  let mut lock = ASSETS.lock().unwrap();

  lock.spot = spot;
  lock.future = future;

  Ok(lock.clone())
}