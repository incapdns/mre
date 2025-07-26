use std::collections::BTreeMap;
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Debug)]
pub struct ValidPair {
  spot_asset: Asset,
  future_asset: Asset,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum MarketType {
  Spot,
  Future,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct Asset {
  #[serde(skip)]
  pub symbol: String,
  pub base: String,
  pub quote: String,
  pub market: MarketType,
  pub exchange: String,
}

#[derive(Debug, Clone)]
pub struct Assets {
  pub spot: BTreeMap<String, Asset>,
  pub future: BTreeMap<String, Asset>,
}

impl Default for Assets {
  fn default() -> Self {
    Self::new()
  }
}

impl Assets {
  pub fn new() -> Self {
    Self {
      spot: BTreeMap::new(),
      future: BTreeMap::new(),
    }
  }
}