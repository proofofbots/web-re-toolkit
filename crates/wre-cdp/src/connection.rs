use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use futures_util::{SinkExt, StreamExt};
use serde::Deserialize;
use serde_json::{Value, json};
use tokio::sync::{Mutex, mpsc, oneshot};
use tokio_tungstenite::tungstenite::Message;

use wre_core::error::{Error, Result};

#[derive(Debug, Clone)]
pub struct Event {
    pub method: String,
    pub params: Value,
    pub session_id: Option<String>,
}

impl Event {
    pub fn param(&self, key: &str) -> Option<&Value> {
        self.params.get(key)
    }

    pub fn string(&self, key: &str) -> Option<&str> {
        self.params.get(key).and_then(Value::as_str)
    }
}

type Pending = Arc<Mutex<HashMap<u64, oneshot::Sender<std::result::Result<Value, String>>>>>;
type Sinks = Arc<Mutex<Vec<mpsc::UnboundedSender<Event>>>>;

#[derive(Clone)]
pub struct Connection {
    outgoing: mpsc::UnboundedSender<Message>,
    pending: Pending,
    sinks: Sinks,
    next_id: Arc<AtomicU64>,
    timeout: Duration,
}

impl Connection {
    pub async fn connect(url: &str) -> Result<Self> {
        let (stream, _) = tokio_tungstenite::connect_async(url)
            .await
            .map_err(|error| Error::msg(format!("cdp websocket connect to {url} failed: {error}")))?;

        let (mut writer, mut reader) = stream.split();
        let (outgoing, mut outbox) = mpsc::unbounded_channel::<Message>();

        let pending: Pending = Arc::new(Mutex::new(HashMap::new()));
        let sinks: Sinks = Arc::new(Mutex::new(Vec::new()));

        tokio::spawn(async move {
            while let Some(message) = outbox.recv().await {
                if writer.send(message).await.is_err() {
                    break;
                }
            }
        });

        let reader_pending = Arc::clone(&pending);
        let reader_sinks = Arc::clone(&sinks);

        tokio::spawn(async move {
            while let Some(message) = reader.next().await {
                let Ok(message) = message else { break };

                let text = match message {
                    Message::Text(text) => text.to_string(),
                    Message::Binary(bytes) => String::from_utf8_lossy(&bytes).into_owned(),
                    Message::Close(_) => break,
                    _ => continue,
                };

                let Ok(frame) = serde_json::from_str::<Frame>(&text) else {
                    tracing::debug!("cdp frame not understood: {}", truncate(&text));
                    continue;
                };

                if let Some(id) = frame.id {
                    let sender = reader_pending.lock().await.remove(&id);
                    if let Some(sender) = sender {
                        let outcome = match frame.error {
                            Some(error) => Err(format!(
                                "{} ({})",
                                error.message,
                                error.data.unwrap_or_default()
                            )),
                            None => Ok(frame.result.unwrap_or(Value::Null)),
                        };
                        let _ = sender.send(outcome);
                    }
                    continue;
                }

                let Some(method) = frame.method else { continue };
                let event = Event {
                    method,
                    params: frame.params.unwrap_or(Value::Null),
                    session_id: frame.session_id,
                };

                let mut guard = reader_sinks.lock().await;
                guard.retain(|sink| sink.send(event.clone()).is_ok());
            }

            let mut guard = reader_pending.lock().await;
            for (_, sender) in guard.drain() {
                let _ = sender.send(Err("cdp connection closed".to_string()));
            }
        });

        Ok(Self {
            outgoing,
            pending,
            sinks,
            next_id: Arc::new(AtomicU64::new(1)),
            timeout: Duration::from_secs(30),
        })
    }

    pub fn with_timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout;
        self
    }

    pub async fn events(&self) -> mpsc::UnboundedReceiver<Event> {
        let (sender, receiver) = mpsc::unbounded_channel();
        self.sinks.lock().await.push(sender);
        receiver
    }

    pub async fn send(
        &self,
        method: &str,
        params: Value,
        session_id: Option<&str>,
    ) -> Result<Value> {
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        let (sender, receiver) = oneshot::channel();
        self.pending.lock().await.insert(id, sender);

        let mut payload = json!({ "id": id, "method": method, "params": params });
        if let Some(session) = session_id {
            payload["sessionId"] = Value::String(session.to_string());
        }

        self.outgoing
            .send(Message::Text(payload.to_string().into()))
            .map_err(|_| Error::msg("cdp connection is closed"))?;

        let outcome = tokio::time::timeout(self.timeout, receiver)
            .await
            .map_err(|_| Error::msg(format!("cdp call {method} timed out")))?
            .map_err(|_| Error::msg(format!("cdp call {method} was dropped")))?;

        outcome.map_err(|message| Error::msg(format!("cdp call {method} failed: {message}")))
    }

    pub async fn call(&self, method: &str, params: Value) -> Result<Value> {
        self.send(method, params, None).await
    }
}

fn truncate(text: &str) -> String {
    if text.len() <= 200 {
        text.to_string()
    } else {
        format!("{}…", &text[..200])
    }
}

#[derive(Debug, Deserialize)]
struct Frame {
    #[serde(default)]
    id: Option<u64>,
    #[serde(default)]
    method: Option<String>,
    #[serde(default)]
    params: Option<Value>,
    #[serde(default)]
    result: Option<Value>,
    #[serde(default)]
    error: Option<FrameError>,
    #[serde(default, rename = "sessionId")]
    session_id: Option<String>,
}

#[derive(Debug, Deserialize)]
struct FrameError {
    message: String,
    #[serde(default)]
    data: Option<String>,
}
