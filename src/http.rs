use std::error::Error;
use std::pin::Pin;
use std::sync::Arc;
use std::time::Duration;
use downcast_rs::{impl_downcast, Downcast};
use ntex::http::{Client, Method, Version};
use ntex::http::client::{ClientResponse, Connector};
use rustls::{ClientConfig, DigitallySignedStruct, RootCertStore, SignatureScheme};
use rustls::client::danger::{HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier};
use rustls::crypto::{verify_tls12_signature, verify_tls13_signature, CryptoProvider};
use rustls::crypto::aws_lc_rs::default_provider;
use rustls::pki_types::{CertificateDer, ServerName, UnixTime};
use thiserror::Error;

// De: src/base/http/generic.rs
pub trait HttpBody: Downcast {}
impl_downcast!(HttpBody);
impl HttpBody for Vec<u8> {}
impl HttpBody for String {}
impl HttpBody for &'static [u8] {}
pub type DynamicIterator<'a, T> = &'a mut dyn Iterator<Item = T>;

pub trait HttpClient {
  type Response;
  fn request<'t, 'b>(
    &'t self,
    method: String,
    uri: String,
    headers: DynamicIterator<'t, (&'t dyn AsRef<str>, &'t dyn AsRef<str>)>,
    body: Option<Box<dyn HttpBody + 'b>>,
  ) -> Pin<Box<dyn Future<Output = Result<Self::Response, Box<dyn std::error::Error>>> + 't>>;
}

#[derive(Debug, Error)]
#[error("Invalid body")]
pub struct BodyError();

// De: src/base/http/client/ntex.rs
pub struct NtexHttpClient {
  client: Client,
}

impl Default for NtexHttpClient {
  fn default() -> Self {
    Self::new()
  }
}
#[derive(Debug)]
pub struct InsecureCertificateVerifier(CryptoProvider);

impl InsecureCertificateVerifier {
  pub fn new(provider: CryptoProvider) -> Self {
    Self(provider)
  }
}

impl ServerCertVerifier for InsecureCertificateVerifier {
  fn verify_server_cert(
    &self,
    _end_entity: &CertificateDer<'_>,
    _intermediates: &[CertificateDer<'_>],
    _server_name: &ServerName<'_>,
    _ocsp: &[u8],
    _now: UnixTime,
  ) -> Result<ServerCertVerified, rustls::Error> {
    Ok(ServerCertVerified::assertion())
  }

  fn verify_tls12_signature(
    &self,
    message: &[u8],
    cert: &CertificateDer<'_>,
    dss: &DigitallySignedStruct,
  ) -> Result<HandshakeSignatureValid, rustls::Error> {
    verify_tls12_signature(
      message,
      cert,
      dss,
      &self.0.signature_verification_algorithms,
    )
  }

  fn verify_tls13_signature(
    &self,
    message: &[u8],
    cert: &CertificateDer<'_>,
    dss: &DigitallySignedStruct,
  ) -> Result<HandshakeSignatureValid, rustls::Error> {
    verify_tls13_signature(
      message,
      cert,
      dss,
      &self.0.signature_verification_algorithms,
    )
  }

  fn supported_verify_schemes(&self) -> Vec<rustls::SignatureScheme> {
    self.0
      .signature_verification_algorithms
      .supported_schemes()
  }
}

impl NtexHttpClient {
  pub fn new() -> Self {
    let connector = Connector::default()
      .timeout(Duration::from_secs(15));

    let protocols = vec![b"h2".to_vec(), b"http/1.1".to_vec()];

    let mut config = ClientConfig::builder()
      .dangerous()
      .with_custom_certificate_verifier(Arc::new(InsecureCertificateVerifier::new(default_provider())))
      .with_no_client_auth();

    config.alpn_protocols = protocols;

    let connector = connector
      .rustls(config)
      .finish();

    Self {
      client: Client::build()
        .connector(connector)
        .finish(),
    }
  }
}

macro_rules! process_downgrade {
    (init, $type:ty, $http_body:ident, $backup:ident) => {{
        let result = $http_body.downcast::<$type>();
        if let Ok(result) = result {
            Some(result)
        } else {
            $backup = Some(result.err().unwrap());
            None
        }
    }};
    (resume, $type:ty, $backup:ident) => {{
        let result = $backup.take().unwrap().downcast::<$type>();
        if let Ok(result) = result {
            Some(result)
        } else {
            $backup = Some(result.err().unwrap());
            None
        }
    }};
}

fn boxed_err<R, E: Error + 'static>(result: Result<R, E>) -> Result<R, Box<dyn Error>> {
  result.map_err(|e| Box::new(e) as Box<dyn Error>)
}

impl HttpClient for NtexHttpClient {
  type Response = ClientResponse;

  fn request<'t, 'b>(
    &'t self,
    method: String,
    uri: String,
    mut headers: DynamicIterator<'t, (&'t dyn AsRef<str>, &'t dyn AsRef<str>)>,
    mut body: Option<Box<dyn HttpBody + 'b>>,
  ) -> Pin<Box<dyn Future<Output = Result<Self::Response, Box<dyn Error>>> + 't>> {
    let client = self.client.clone();
    Box::pin(async move {
      let method = method.parse::<Method>()?;
      let mut req = client
        .request(method, uri)
        .version(Version::HTTP_2)
        .timeout(Duration::from_secs(10));
      for (k, v) in &mut headers {
        req = req.header(k.as_ref(), v.as_ref());
      }
      if let Some(http_body) = body.take() {
        let mut backup: Option<Box<dyn HttpBody + 'b>> = None;
        if let Some(boxed) = process_downgrade!(init, Vec<u8>, http_body, backup) {
          boxed_err(req.send_body(*boxed).await)
        } else if let Some(boxed) = process_downgrade!(resume, String, backup) {
          boxed_err(req.send_body(*boxed).await)
        } else if let Some(boxed) = process_downgrade!(resume, &'static [u8], backup) {
          boxed_err(req.send_body(*boxed).await)
        } else {
          Err(Box::new(BodyError()) as Box<dyn Error>)
        }
      } else {
        boxed_err(req.send().await)
      }
    })
  }
}