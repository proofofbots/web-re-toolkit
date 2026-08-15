use std::time::Duration;

use reqwest::header::{HeaderMap, HeaderName, HeaderValue};
use serde::{Deserialize, Serialize};

use wre_core::error::{Error, Result};

use crate::proxy::ProxySpec;

pub const CHROME_UA: &str = "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/140.0.0.0 Safari/537.36";

#[derive(Debug, Clone)]
pub struct ClientOptions {
    pub proxy: Option<ProxySpec>,
    pub user_agent: String,
    pub timeout: Duration,
    pub accept_invalid_certs: bool,
    pub http2_only: bool,
    pub cookies: bool,
    pub redirects: usize,
}

impl Default for ClientOptions {
    fn default() -> Self {
        Self {
            proxy: None,
            user_agent: CHROME_UA.to_string(),
            timeout: Duration::from_secs(30),
            accept_invalid_certs: false,
            http2_only: false,
            cookies: false,
            redirects: 10,
        }
    }
}

#[derive(Debug, Clone)]
pub struct Client {
    inner: reqwest::Client,
    options: ClientOptions,
}

impl Client {
    pub fn new(options: ClientOptions) -> Result<Self> {
        let mut builder = reqwest::Client::builder()
            .user_agent(options.user_agent.clone())
            .timeout(options.timeout)
            .danger_accept_invalid_certs(options.accept_invalid_certs)
            .redirect(if options.redirects == 0 {
                reqwest::redirect::Policy::none()
            } else {
                reqwest::redirect::Policy::limited(options.redirects)
            });

        if options.http2_only {
            builder = builder.http2_prior_knowledge();
        }

        if let Some(proxy) = &options.proxy {
            let configured = reqwest::Proxy::all(proxy.url())
                .map_err(|error| Error::msg(format!("proxy rejected: {error}")))?;
            builder = builder.proxy(configured);
        }

        let inner = builder
            .build()
            .map_err(|error| Error::msg(format!("http client build failed: {error}")))?;

        Ok(Self { inner, options })
    }

    pub fn plain() -> Result<Self> {
        Self::new(ClientOptions::default())
    }

    pub fn with_proxy(proxy: Option<ProxySpec>) -> Result<Self> {
        Self::new(ClientOptions { proxy, ..ClientOptions::default() })
    }

    pub fn raw(&self) -> &reqwest::Client {
        &self.inner
    }

    pub fn options(&self) -> &ClientOptions {
        &self.options
    }

    pub async fn get_text(&self, url: &str) -> Result<String> {
        let response = self
            .inner
            .get(url)
            .send()
            .await
            .map_err(|error| Error::msg(format!("GET {url} failed: {error}")))?;

        response
            .text()
            .await
            .map_err(|error| Error::msg(format!("GET {url} body failed: {error}")))
    }

    pub async fn get_bytes(&self, url: &str) -> Result<Vec<u8>> {
        let response = self
            .inner
            .get(url)
            .send()
            .await
            .map_err(|error| Error::msg(format!("GET {url} failed: {error}")))?;

        Ok(response
            .bytes()
            .await
            .map_err(|error| Error::msg(format!("GET {url} body failed: {error}")))?
            .to_vec())
    }

    pub async fn fetch(&self, request: FetchRequest) -> Result<FetchResponse> {
        let method = reqwest::Method::from_bytes(request.method.as_bytes())
            .map_err(|_| Error::msg(format!("bad method {}", request.method)))?;

        let mut builder = self.inner.request(method, &request.url);
        builder = builder.headers(header_map(&request.headers)?);

        if let Some(body) = request.body {
            builder = builder.body(body);
        }

        let response = builder
            .send()
            .await
            .map_err(|error| Error::msg(format!("{} {} failed: {error}", request.method, request.url)))?;

        let status = response.status().as_u16();
        let final_url = response.url().to_string();
        let version = format!("{:?}", response.version());

        let mut headers = Vec::new();
        for (name, value) in response.headers() {
            headers.push((name.to_string(), value.to_str().unwrap_or_default().to_string()));
        }

        let body = response
            .bytes()
            .await
            .map_err(|error| Error::msg(format!("body read failed: {error}")))?
            .to_vec();

        Ok(FetchResponse { status, url: final_url, version, headers, body })
    }

    pub async fn public_address(&self) -> Result<String> {
        let text = self.get_text("https://api.ipify.org?format=json").await?;
        let parsed: serde_json::Value = serde_json::from_str(&text)
            .map_err(|error| Error::msg(format!("ipify returned non json: {error}")))?;
        parsed
            .get("ip")
            .and_then(|value| value.as_str())
            .map(str::to_string)
            .ok_or_else(|| Error::msg("ipify response has no ip field"))
    }
}

fn header_map(headers: &[(String, String)]) -> Result<HeaderMap> {
    let mut map = HeaderMap::new();
    for (name, value) in headers {
        let name = HeaderName::from_bytes(name.as_bytes())
            .map_err(|_| Error::msg(format!("bad header name {name}")))?;
        let value = HeaderValue::from_str(value)
            .map_err(|_| Error::msg(format!("bad header value for {name}")))?;
        map.append(name, value);
    }
    Ok(map)
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct FetchRequest {
    pub url: String,
    #[serde(default = "get_method")]
    pub method: String,
    #[serde(default)]
    pub headers: Vec<(String, String)>,
    #[serde(default)]
    pub body: Option<Vec<u8>>,
}

fn get_method() -> String {
    "GET".to_string()
}

impl FetchRequest {
    pub fn get(url: impl Into<String>) -> Self {
        Self { url: url.into(), method: get_method(), headers: Vec::new(), body: None }
    }

    pub fn post(url: impl Into<String>, body: Vec<u8>) -> Self {
        Self {
            url: url.into(),
            method: "POST".to_string(),
            headers: Vec::new(),
            body: Some(body),
        }
    }

    pub fn header(mut self, name: impl Into<String>, value: impl Into<String>) -> Self {
        self.headers.push((name.into(), value.into()));
        self
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FetchResponse {
    pub status: u16,
    pub url: String,
    pub version: String,
    pub headers: Vec<(String, String)>,
    #[serde(with = "body_serde")]
    pub body: Vec<u8>,
}

impl FetchResponse {
    pub fn text(&self) -> String {
        String::from_utf8_lossy(&self.body).into_owned()
    }

    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(key, _)| key.eq_ignore_ascii_case(name))
            .map(|(_, value)| value.as_str())
    }

    pub fn set_cookies(&self) -> Vec<&str> {
        self.headers
            .iter()
            .filter(|(key, _)| key.eq_ignore_ascii_case("set-cookie"))
            .map(|(_, value)| value.as_str())
            .collect()
    }
}

mod body_serde {
    use base64::Engine;
    use base64::engine::general_purpose::STANDARD;
    use serde::{Deserialize, Deserializer, Serializer};

    pub fn serialize<S: Serializer>(bytes: &[u8], serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&STANDARD.encode(bytes))
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Vec<u8>, D::Error> {
        let text = String::deserialize(deserializer)?;
        STANDARD.decode(text).map_err(serde::de::Error::custom)
    }
}
