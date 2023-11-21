use anyhow::Context;
use futures::future::BoxFuture;
use std::convert::TryFrom;
use tokio::runtime::Handle;

use super::{HttpRequest, HttpResponse};

#[derive(Clone, Debug)]
pub struct ReqwestHttpClient {
    handle: Handle,
}
impl Default for ReqwestHttpClient {
    fn default() -> Self {
        Self {
            handle: Handle::current(),
        }
    }
}

impl ReqwestHttpClient {
    async fn request(&self, request: HttpRequest) -> Result<HttpResponse, anyhow::Error> {
        let method = reqwest::Method::try_from(request.method.as_str())
            .with_context(|| format!("Invalid http method {}", request.method))?;

        // TODO: use persistent client?
        let client = {
            let _guard = Handle::try_current().map_err(|_| self.handle.enter());
            reqwest::ClientBuilder::default()
                .build()
                .context("Could not create reqwest client")?
        };

        let mut builder = client.request(method, request.url.as_str());
        for (header, val) in &request.headers {
            builder = builder.header(header, val);
        }

        if let Some(body) = request.body {
            builder = builder.body(reqwest::Body::from(body));
        }

        let request = builder
            .build()
            .context("Failed to construct http request")?;

        let mut response = client.execute(request).await?;
        let headers = std::mem::take(response.headers_mut());

        let status = response.status();
        let data = response.bytes().await?.to_vec();

        Ok(HttpResponse {
            status,
            redirected: false,
            body: Some(data),
            headers,
        })
    }
}

impl super::HttpClient for ReqwestHttpClient {
    fn request(&self, request: HttpRequest) -> BoxFuture<'_, Result<HttpResponse, anyhow::Error>> {
        let client = self.clone();
        let f = async move { client.request(request).await };
        Box::pin(f)
    }
}
