use crate::gate::auxiliar::coditional_future::ConditionalFuture;
use crate::http::{DynamicIterator, NtexHttpClient};
use std::cell::RefCell;
use std::cmp::Reverse;
use std::collections::{HashMap, HashSet};
use std::error::Error;
use std::mem;
use std::rc::Rc;
use std::sync::{Arc, LazyLock, Mutex};
use std::time::Duration;
use futures::stream::FuturesUnordered;
use futures::{join, StreamExt};
use ntex::connect::rustls::TlsConnector;
use ntex::io::Sealed;
use ntex::ws;
use ntex::ws::{WsConnection, WsSink};
use once_cell::sync::OnceCell;
use ratelimit::Ratelimiter;
use rust_decimal::Decimal;
use rustls::RootCertStore;
use serde::Deserialize;
use crate::gate::auxiliar::asset::{Assets, MarketType};
use crate::gate::order::{OrderBook, OrderBookUpdate};
use crate::gate::auxiliar::utils::{normalize_symbol, GateExchangeUtils};
use crate::project;

pub mod auxiliar;
mod order;

#[macro_export]
macro_rules! from_headers {
  ($headers:expr) => {
    &mut $headers
      .iter()
      .map(|(k, v)| (k as &dyn AsRef<str>, v as &dyn AsRef<str>))
    as DynamicIterator<(&dyn AsRef<str>, &dyn AsRef<str>)>
  };
}

#[derive(Debug, Deserialize)]
struct SpotDepthSnapshot {
  id: u64,
  bids: Vec<(Decimal, Decimal)>,
  asks: Vec<(Decimal, Decimal)>,
}

#[derive(Debug, Deserialize)]
struct FutureDepthSnapshotItem {
  p: Decimal,
  s: Decimal,
}

#[derive(Debug, Deserialize)]
struct FutureDepthSnapshot {
  id: u64,
  bids: Vec<FutureDepthSnapshotItem>,
  asks: Vec<FutureDepthSnapshotItem>,
}

type Init = Rc<RefCell<HashMap<String, Vec<OrderBookUpdate>>>>;
pub type SharedBook = Rc<RefCell<OrderBook>>;
pub type HistoryBids = HashSet<Decimal>;

pub struct Gate {
  market: MarketType,
  subscribed: Rc<RefCell<HashMap<String, SharedBook>>>,
  history_bids: Rc<RefCell<HashMap<String, HistoryBids>>>,
  init: Init,
  sink: WsSink
}

static ASSETS: LazyLock<Mutex<Assets>> = LazyLock::new(|| Mutex::new(Assets::new()));

pub static CONNECT_LIMITER: OnceCell<Arc<Ratelimiter>> = OnceCell::new();
pub static HTTP_LIMITER: OnceCell<Arc<Ratelimiter>> = OnceCell::new();


impl Gate {
  pub async fn new(ws_url: String, market: MarketType, utils: Rc<GateExchangeUtils>) -> Self {
    let connector = {
      let root_store = RootCertStore {
        roots: webpki_roots::TLS_SERVER_ROOTS.into(),
      };

      let config = rustls::ClientConfig::builder()
        .with_root_certificates(root_store)
        .with_no_client_auth();

      TlsConnector::new(config)
    };

    let subscribed = Rc::new(RefCell::new(HashMap::new()));
    let init_queue = Rc::new(RefCell::new(HashMap::new()));
    let history_bids = Rc::new(RefCell::new(HashMap::new()));;

    let ws_client = ws::WsClient::build(ws_url)
      .connector(connector)
      .finish()
      .map_err(|e| format!("Build error: {e:?}"))
      .unwrap()
      .connect()
      .await
      .map_err(|e| format!("Connect error: {e:?}"))
      .unwrap();

    let sealed = ws_client.seal();

    let sink = sealed.sink();
    let mut rx_ws = sealed.receiver();


    let sink_cl = sink.clone();
    let init_queue_cl = init_queue.clone();
    let subscribed_cl = subscribed.clone();
    let utils_cl = utils.clone();
    let market_cl = market.clone();
    let history_bids_cl = history_bids.clone();

    ntex::rt::spawn(async move {
      let mut client_tasks = FuturesUnordered::new();
      let history_bids = history_bids_cl.clone();

      loop {
        macros::select! {
          message = rx_ws.next() => match message {
            Some(Ok(ws::Frame::Text(bytes))) => {
              let str = String::from_utf8(bytes.to_vec());
              let task = Box::pin(Self::handle_message(str.ok().unwrap(), subscribed_cl.clone(), history_bids.clone(), init_queue_cl.clone(), utils_cl.clone(), market_cl.clone()));

              let task_projection = project!(task);

              if task_projection.is_err() {
                client_tasks.push(task_projection.err().unwrap());
              }
            }
            Some(Ok(ws::Frame::Ping(p))) => {
              let _ = sink_cl.send(ws::Message::Pong(p)).await;
            }
            Some(Err(e)) => {
              eprintln!("WebSocket Error: {:?}", e);
              break;
            }
            _ => {}
          },
          client_res = client_tasks.next(), if !client_tasks.is_empty() => {
            if client_res.is_none() {
              break;
            }
          }
        }
      }
    });

    Self {
      market,
      subscribed,
      init: init_queue,
      sink,
      history_bids
    }
  }

  pub async fn watch(&self, symbol: String) -> Result<(), Box<dyn Error>> {
    let normalized_symbol = normalize_symbol(&symbol);

    let sink = self.sink.clone();

    // Subscribe
    let msg = serde_json::json!({
          "time": std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_secs(),
          "channel": if self.market == MarketType::Spot { "spot.order_book_update" } else { "futures.order_book_update" },
          "event": "subscribe",
          "payload": [&normalized_symbol, "100ms"]
        });
    sink.send(ws::Message::Text(msg.to_string().into())).await?;

    let book = Rc::new(RefCell::new(OrderBook::default()));
    self.subscribed.borrow_mut().insert(normalized_symbol.clone(), book);

    let history_bids = HistoryBids::new();
    self.history_bids.borrow_mut().insert(normalized_symbol.clone(), history_bids);

    let init = Vec::with_capacity(1000);
    self.init.borrow_mut().insert(normalized_symbol.clone(), init);



    Ok(())
  }

  async fn handle_message(
    text: String,
    subscribed: Rc<RefCell<HashMap<String, SharedBook>>>,
    history_bids: Rc<RefCell<HashMap<String, HistoryBids>>>,
    init: Init,
    utils: Rc<GateExchangeUtils>,
    market: MarketType,
  ) -> Option<()> {
    let Ok(parsed_value) = serde_json::from_str::<serde_json::Value>(&text) else { return None };

    let valid = parsed_value.get("event").and_then(|e| e.as_str()) == Some("update");

    if !valid {
      return None
    }

    let result = parsed_value.get("result").unwrap();
    let symbol = result["s"].as_str().unwrap().to_string();

    let book = {
      let s = subscribed.borrow();
      let book = s.get(&symbol).unwrap();
      book.clone()
    };

    let update = Self::build_update(result, &market).unwrap();

    if book.borrow().update_id == 0 { // First update
      if update.full {
        book.borrow_mut().apply_update(&update, &mut vec![]);
        return Some(());
      }

      book.borrow_mut().update_id = 1;

      let snapshot =
        Self::fetch_snapshot(&symbol, utils.clone(), update.first_update_id, market.clone()).await;

      if snapshot.is_err() {
        book.borrow_mut().update_id = 0;
        return Some(());
      }

      let snapshot = snapshot.ok()?;

      let mut retries = 0;
      let processed = loop {
        if retries == 5 {
          book.borrow_mut().update_id = 0;
          return Some(());
        }

        let mut pending = mem::take(init.borrow_mut().get_mut(&symbol)?);

        let idx = pending.iter().position(|item| {
          item.first_update_id <= snapshot.update_id + 1
            && item.last_update_id > snapshot.update_id
        });

        if idx.is_none() {
          ntex::time::sleep(Duration::from_millis(100)).await;
          retries += 1;
          continue;
        }

        pending.drain(0..idx.unwrap());

        break pending;
      };

      let mut book_bm = book.borrow_mut();
      book_bm.asks = snapshot.asks;
      book_bm.bids = snapshot.bids;
      book_bm.update_id = snapshot.update_id;

      let mut history_bm = history_bids.borrow_mut();
      let history = history_bm
        .get_mut(&symbol)?;

      for (price, qty) in &book_bm.bids {
        if qty.eq(&Decimal::ZERO) {
          history.remove(&price.0);
        } else {
          history.insert(price.0);
        }
      }

      let mut updates = vec![];
      for update in processed {
        if update.first_update_id > book_bm.update_id + 1
          || update.last_update_id < book_bm.update_id + 1
        {
          book_bm.update_id = 0;
          break;
        }
        book_bm.apply_update(&update, &mut updates);
        for (price, qty) in &update.bids {
          if qty.eq(&Decimal::ZERO) {
            history.remove(&price);
          } else {
            history.insert(*price);
          }
        }
      };
    } else if book.borrow().update_id == 1 { // Initializing
      init.borrow_mut().get_mut(&symbol).unwrap().push(update);
    } else { // Normal update
      {
        let mut book_mut = book.borrow_mut();
        if update.first_update_id > book_mut.update_id + 1 || update.last_update_id < book_mut.update_id + 1 {
          book_mut.asks.clear();
          book_mut.bids.clear();
          book_mut.update_id = 0;
          return Some(());
        }
      }
      book.borrow_mut().apply_update(&update, &mut vec![]);

      let mut history_bm = history_bids.borrow_mut();
      let history = history_bm
              .get_mut(&symbol)?;

      for (price, qty) in &update.bids {
        if qty.eq(&Decimal::ZERO) {
          history.remove(&price);
        } else {
          history.insert(*price);
        }
      }

      for (r_bid, _) in book.borrow().bids.iter() {
        if history.get(&r_bid.0).is_none() {
          panic!("Error in BID price {:?}", r_bid);
        }
      }
    }

    Some(())
  }

  fn build_update(parsed: &serde_json::Value, market: &MarketType) -> Option<OrderBookUpdate> {
    let parse_side = |side: &serde_json::Value| -> Option<Vec<(Decimal, Decimal)>> {
      side.as_array()?.iter().map(|entry| {
        if *market == MarketType::Spot {
          let price = entry[0].as_str()?.parse().ok()?;
          let qty = entry[1].as_str()?.parse().ok()?;
          Some((price, qty))
        } else {
          let item: FutureDepthSnapshotItem = serde_json::from_value(entry.clone()).ok()?;
          Some((item.p, item.s))
        }
      }).collect()
    };

    Some(OrderBookUpdate {
      asks: parse_side(&parsed["a"])?,
      bids: parse_side(&parsed["b"])?,
      first_update_id: parsed["U"].as_u64()?,
      last_update_id: parsed["u"].as_u64()?,
      full: parsed["full"].as_bool().unwrap_or(false),
    })
  }

  async fn fetch_snapshot(
    symbol: &str,
    utils: Rc<GateExchangeUtils>,
    initial_event_u: u64,
    market: MarketType,
  ) -> Result<OrderBook, Box<dyn Error>> {
    let mut processed = OrderBook::default();

    let mut retries = 0;

    while processed.update_id < initial_event_u {
      if retries == 5 {
        return Err("Max retry".into());
      }

      let uri = match market {
        MarketType::Spot => {
          format!(
            "https://api.gateio.ws/api/v4/spot/order_book?currency_pair={symbol}&limit=100&with_id=true"
          )
        }
        MarketType::Future => {
          format!(
            "https://fx-api.gateio.ws/api/v4/futures/usdt/order_book?contract={symbol}&limit=100&with_id=true"
          )
        }
      };

      let headers = from_headers!([("Accept", "application/json")]);

      let response = loop {
        match HTTP_LIMITER
          .get()
          .expect("Limiter not initialized")
          .try_wait()
        {
          Ok(()) => {
            break utils
              .http_client
              .request("GET".into(), uri, headers, None)
              .await?
              .body()
              .limit(10 * 1024 * 1024)
              .await?;
          }
          Err(duration) => {
            ntex::time::sleep(duration).await;
          }
        }
      };

      if let MarketType::Future = market {
        let result = serde_json::from_slice::<FutureDepthSnapshot>(&response)?;

        processed = OrderBook {
          asks: result
            .asks
            .into_iter()
            .map(|item| (item.p, item.s))
            .collect(),
          bids: result
            .bids
            .into_iter()
            .map(|item| (Reverse(item.p), item.s))
            .collect(),
          update_id: result.id,
        };
      } else {
        let result = serde_json::from_slice::<SpotDepthSnapshot>(&response)?;

        processed = OrderBook {
          asks: result.asks.into_iter().collect(),
          bids: result
            .bids
            .into_iter()
            .map(|(k, v)| (Reverse(k), v))
            .collect(),
          update_id: result.id,
        };
      }

      retries += 1;
    }

    Ok(processed)
  }
}