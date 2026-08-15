use std::time::Duration;

use serde::de::DeserializeOwned;
use serde_json::{Value, json};
use tokio::sync::mpsc;

use wre_core::error::{Error, Result};

use crate::connection::{Connection, Event};

#[derive(Clone)]
pub struct Session {
    connection: Connection,
    pub session_id: String,
    pub target_id: String,
}

impl Session {
    pub fn new(connection: Connection, session_id: String, target_id: String) -> Self {
        Self { connection, session_id, target_id }
    }

    pub fn connection(&self) -> &Connection {
        &self.connection
    }

    pub async fn send(&self, method: &str, params: Value) -> Result<Value> {
        self.connection
            .send(method, params, Some(&self.session_id))
            .await
    }

    pub async fn try_send(&self, method: &str, params: Value) -> Option<Value> {
        match self.send(method, params).await {
            Ok(value) => Some(value),
            Err(error) => {
                tracing::debug!("{method} ignored: {error}");
                None
            }
        }
    }

    pub async fn events(&self) -> mpsc::UnboundedReceiver<Event> {
        let mut source = self.connection.events().await;
        let (sender, receiver) = mpsc::unbounded_channel();
        let session_id = self.session_id.clone();

        tokio::spawn(async move {
            while let Some(event) = source.recv().await {
                let matches = event
                    .session_id
                    .as_ref()
                    .map(|id| *id == session_id)
                    .unwrap_or(true);
                if matches && sender.send(event).is_err() {
                    break;
                }
            }
        });

        receiver
    }

    pub async fn enable(&self, domains: &[&str]) -> Result<()> {
        for domain in domains {
            self.send(&format!("{domain}.enable"), json!({})).await?;
        }
        Ok(())
    }

    pub async fn navigate(&self, url: &str) -> Result<()> {
        let result = self.send("Page.navigate", json!({ "url": url })).await?;
        if let Some(error) = result.get("errorText").and_then(Value::as_str) {
            return Err(Error::msg(format!("navigation to {url} failed: {error}")));
        }
        Ok(())
    }

    pub async fn wait_for_event(
        &self,
        method: &str,
        timeout: Duration,
    ) -> Result<Event> {
        let mut events = self.events().await;
        let deadline = tokio::time::sleep(timeout);
        tokio::pin!(deadline);

        loop {
            tokio::select! {
                event = events.recv() => {
                    let Some(event) = event else {
                        return Err(Error::msg("cdp event stream ended"));
                    };
                    if event.method == method {
                        return Ok(event);
                    }
                }
                _ = &mut deadline => {
                    return Err(Error::msg(format!("timed out waiting for {method}")));
                }
            }
        }
    }

    pub async fn navigate_and_wait(&self, url: &str, timeout: Duration) -> Result<()> {
        let mut events = self.events().await;
        self.navigate(url).await?;

        let deadline = tokio::time::sleep(timeout);
        tokio::pin!(deadline);

        loop {
            tokio::select! {
                event = events.recv() => {
                    let Some(event) = event else { return Ok(()) };
                    if event.method == "Page.loadEventFired" {
                        return Ok(());
                    }
                }
                _ = &mut deadline => return Ok(()),
            }
        }
    }

    pub async fn evaluate(&self, expression: &str) -> Result<Value> {
        let result = self
            .send(
                "Runtime.evaluate",
                json!({
                    "expression": expression,
                    "returnByValue": true,
                    "awaitPromise": true,
                    "allowUnsafeEvalBlockedByCSP": true,
                }),
            )
            .await?;

        if let Some(details) = result.get("exceptionDetails") {
            let text = details
                .get("exception")
                .and_then(|value| value.get("description"))
                .and_then(Value::as_str)
                .or_else(|| details.get("text").and_then(Value::as_str))
                .unwrap_or("evaluation threw");
            return Err(Error::msg(format!("page evaluation failed: {text}")));
        }

        Ok(result
            .get("result")
            .and_then(|value| value.get("value"))
            .cloned()
            .unwrap_or(Value::Null))
    }

    pub async fn evaluate_as<T: DeserializeOwned>(&self, expression: &str) -> Result<T> {
        let value = self.evaluate(expression).await?;
        serde_json::from_value(value)
            .map_err(|error| Error::msg(format!("page result did not deserialize: {error}")))
    }

    pub async fn evaluate_json(&self, expression: &str) -> Result<Value> {
        let wrapped = format!("JSON.stringify((() => {{ return ({expression}); }})())");
        let value = self.evaluate(&wrapped).await?;
        match value {
            Value::String(text) => serde_json::from_str(&text)
                .map_err(|error| Error::msg(format!("page returned invalid json: {error}"))),
            other => Ok(other),
        }
    }

    pub async fn add_init_script(&self, source: &str) -> Result<String> {
        let result = self
            .send(
                "Page.addScriptToEvaluateOnNewDocument",
                json!({ "source": source, "runImmediately": true }),
            )
            .await?;

        Ok(result
            .get("identifier")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string())
    }

    pub async fn remove_init_script(&self, identifier: &str) -> Result<()> {
        self.send(
            "Page.removeScriptToEvaluateOnNewDocument",
            json!({ "identifier": identifier }),
        )
        .await?;
        Ok(())
    }

    pub async fn response_body(&self, request_id: &str) -> Result<Vec<u8>> {
        let result = self
            .send("Network.getResponseBody", json!({ "requestId": request_id }))
            .await?;
        decode_body(&result)
    }

    pub async fn request_post_data(&self, request_id: &str) -> Result<String> {
        let result = self
            .send("Network.getRequestPostData", json!({ "requestId": request_id }))
            .await?;
        Ok(result
            .get("postData")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string())
    }

    pub async fn cookies(&self) -> Result<Vec<Value>> {
        let result = self.send("Network.getAllCookies", json!({})).await?;
        Ok(result
            .get("cookies")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default())
    }

    pub async fn set_cookies(&self, cookies: Vec<Value>) -> Result<()> {
        self.send("Network.setCookies", json!({ "cookies": cookies }))
            .await?;
        Ok(())
    }

    pub async fn clear_browser_state(&self, origins: &[String]) -> Result<()> {
        self.try_send("Network.clearBrowserCookies", json!({})).await;
        self.try_send("Network.clearBrowserCache", json!({})).await;

        for origin in origins {
            self.try_send(
                "Storage.clearDataForOrigin",
                json!({ "origin": origin, "storageTypes": "all" }),
            )
            .await;
        }

        Ok(())
    }

    pub async fn document_html(&self) -> Result<String> {
        let value = self.evaluate("document.documentElement.outerHTML").await?;
        Ok(value.as_str().unwrap_or_default().to_string())
    }

    pub async fn frame_tree(&self) -> Result<Value> {
        self.send("Page.getFrameTree", json!({})).await
    }

    pub async fn set_cache_disabled(&self, disabled: bool) -> Result<()> {
        self.send(
            "Network.setCacheDisabled",
            json!({ "cacheDisabled": disabled }),
        )
        .await?;
        Ok(())
    }

    pub async fn set_extra_headers(&self, headers: Value) -> Result<()> {
        self.send(
            "Network.setExtraHTTPHeaders",
            json!({ "headers": headers }),
        )
        .await?;
        Ok(())
    }

    pub async fn set_buffer_sizes(&self) -> Result<()> {
        self.send(
            "Network.enable",
            json!({
                "maxTotalBufferSize": 250_000_000u64,
                "maxResourceBufferSize": 100_000_000u64,
                "maxPostDataSize": 20_000_000u64,
            }),
        )
        .await?;
        Ok(())
    }

    pub async fn detach(&self) -> Result<()> {
        self.connection
            .call(
                "Target.detachFromTarget",
                json!({ "sessionId": self.session_id }),
            )
            .await?;
        Ok(())
    }
}

pub fn decode_body(result: &Value) -> Result<Vec<u8>> {
    use base64::Engine;
    use base64::engine::general_purpose::STANDARD;

    let body = result
        .get("body")
        .and_then(Value::as_str)
        .unwrap_or_default();

    let encoded = result
        .get("base64Encoded")
        .and_then(Value::as_bool)
        .unwrap_or(false);

    if encoded {
        STANDARD
            .decode(body)
            .map_err(|error| Error::msg(format!("cdp body base64 decode failed: {error}")))
    } else {
        Ok(body.as_bytes().to_vec())
    }
}
