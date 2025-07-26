use ntex::http::client::ClientResponse;
use crate::http::HttpClient;

pub struct GateExchangeUtils {
  pub http_client: Box<dyn HttpClient<Response = ClientResponse>>,
}

impl GateExchangeUtils {
  pub fn new<C>(http_client: C) -> Self
  where
    C: HttpClient<Response = ClientResponse> + 'static,
  {
    Self {
      http_client: Box::new(http_client),
    }
  }
}
pub fn normalize_symbol(symbol: &str) -> String {
  let before = match symbol.find(':') {
    Some(pos) => &symbol[..pos],
    None => symbol,
  };
  before.replace('/', "_")
}