use std::env::set_var;
use std::net;
use std::rc::Rc;
use std::time::Duration;
use futures::stream::FuturesUnordered;
use futures::StreamExt;
use futures::future::join_all;
use rustls::crypto::aws_lc_rs::default_provider;
use crate::gate::auxiliar::asset::MarketType;
use crate::gate::auxiliar::fetch::fetch_assets;
use crate::gate::auxiliar::sync::sync_time;
use crate::gate::auxiliar::utils::GateExchangeUtils;
use crate::gate::Gate;
use crate::http::NtexHttpClient;

pub mod gate;
mod http;

#[ntex::main]
async fn main() -> std::io::Result<()> {
  env_logger::init();

  log::warn!("[root] warn");
  log::info!("[root] info");
  log::debug!("[root] debug");

  default_provider()
    .install_default()
    .expect("Failed to install default CryptoProvider");

  let res = net::ToSocketAddrs::to_socket_addrs("api.gateio.ws");
  println!("{:?}", res);

  let res = net::ToSocketAddrs::to_socket_addrs("api.gateio.ws:443");
  println!("{:?}", res);
  let utils = Rc::new(GateExchangeUtils::new(NtexHttpClient::new()));

  sync_time(utils.clone()).await.expect("Sync time failed");

  let gates = join_all(vec![
      Gate::new("wss://fx-ws.gateio.ws/v4/ws/usdt".to_string(), MarketType::Future, utils.clone()),
      Gate::new("wss://fx-ws.gateio.ws/v4/ws/usdt".to_string(), MarketType::Future, utils.clone()),
      Gate::new("wss://fx-ws.gateio.ws/v4/ws/usdt".to_string(), MarketType::Future, utils.clone()),
      Gate::new("wss://fx-ws.gateio.ws/v4/ws/usdt".to_string(), MarketType::Future, utils.clone()),
      Gate::new("wss://fx-ws.gateio.ws/v4/ws/usdt".to_string(), MarketType::Future, utils.clone()),
      Gate::new("wss://fx-ws.gateio.ws/v4/ws/usdt".to_string(), MarketType::Future, utils.clone()),
      Gate::new("wss://fx-ws.gateio.ws/v4/ws/usdt".to_string(), MarketType::Future, utils.clone()),
      Gate::new("wss://fx-ws.gateio.ws/v4/ws/usdt".to_string(), MarketType::Future, utils.clone()),
      Gate::new("wss://fx-ws.gateio.ws/v4/ws/usdt".to_string(), MarketType::Future, utils.clone()),
      Gate::new("wss://fx-ws.gateio.ws/v4/ws/usdt".to_string(), MarketType::Future, utils.clone()),
      Gate::new("wss://fx-ws.gateio.ws/v4/ws/usdt".to_string(), MarketType::Future, utils.clone()),
      Gate::new("wss://fx-ws.gateio.ws/v4/ws/usdt".to_string(), MarketType::Future, utils.clone()),
      Gate::new("wss://fx-ws.gateio.ws/v4/ws/usdt".to_string(), MarketType::Future, utils.clone()),
      Gate::new("wss://fx-ws.gateio.ws/v4/ws/usdt".to_string(), MarketType::Future, utils.clone()),
      Gate::new("wss://fx-ws.gateio.ws/v4/ws/usdt".to_string(), MarketType::Future, utils.clone()),
      Gate::new("wss://fx-ws.gateio.ws/v4/ws/usdt".to_string(), MarketType::Future, utils.clone())
    ]
  ).await;

  let assets = fetch_assets(utils).await.expect("Fetch assets failed");

  let mut tasks = FuturesUnordered::new();

  let mut i = 0;
  for (symbol, _) in assets.future {
    let gate = &gates[i % 16];
    tasks.push(gate.watch(symbol.clone()));
    i += 1;
  }

  while let Some(_) = tasks.next().await {}

  loop {
    ntex::time::sleep(Duration::from_secs(5)).await;
  }

  Ok(())
}