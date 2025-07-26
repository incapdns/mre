use std::pin::Pin;
use std::task::{Context, Poll};

pub struct ConditionalFuture<F: Future + Unpin> {
  inner_future: Option<F>,
}

impl<F: Future + Unpin> ConditionalFuture<F> {
  pub fn new(future: F) -> Self {
    ConditionalFuture {
      inner_future: Some(future),
    }
  }
}

impl<F: Future + Unpin> Future for ConditionalFuture<F> {
  type Output = Result<F::Output, F>;

  fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
    let mut this = self.as_mut();

    let mut inner_future = this.inner_future.take().unwrap();
    let pinned = Pin::new(&mut inner_future);

    match pinned.poll(cx) {
      Poll::Ready(val) => Poll::Ready(Ok(val)),
      Poll::Pending => Poll::Ready(Err(inner_future)),
    }
  }
}

#[macro_export]
macro_rules! project {
  ($self:ident) => {{
    ConditionalFuture::new($self).await
  }};
}