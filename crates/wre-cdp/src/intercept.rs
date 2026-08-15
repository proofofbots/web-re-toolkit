use std::sync::Arc;

use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use serde_json::{Value, json};
use tokio::task::JoinHandle;

use wre_core::error::Result;

use crate::session::{Session, decode_body};

#[derive(Debug, Clone)]
pub struct PausedRequest {
    pub request_id: String,
    pub url: String,
    pub method: String,
    pub resource_type: String,
    pub status: Option<u16>,
    pub request_headers: Vec<(String, String)>,
    pub response_headers: Vec<(String, String)>,
    pub post_data: Option<String>,
    pub body: Option<Vec<u8>>,
}

impl PausedRequest {
    pub fn is_response_stage(&self) -> bool {
        self.status.is_some()
    }

    pub fn text(&self) -> Option<String> {
        self.body
            .as_ref()
            .map(|bytes| String::from_utf8_lossy(bytes).into_owned())
    }
}

#[derive(Debug, Clone)]
pub enum Decision {
    Continue,
    Fulfill {
        status: u16,
        headers: Vec<(String, String)>,
        body: Vec<u8>,
    },
    Fail(String),
}

impl Decision {
    pub fn serve(request: &PausedRequest, body: Vec<u8>) -> Self {
        Decision::Fulfill {
            status: request.status.unwrap_or(200),
            headers: request
                .response_headers
                .iter()
                .filter(|(name, _)| {
                    !name.eq_ignore_ascii_case("content-encoding")
                        && !name.eq_ignore_ascii_case("content-length")
                })
                .cloned()
                .collect(),
            body,
        }
    }
}

pub type Handler = Arc<dyn Fn(&PausedRequest) -> Decision + Send + Sync>;

#[derive(Debug, Clone)]
pub struct InterceptOptions {
    pub patterns: Vec<Value>,
    pub body_types: Vec<String>,
}

impl Default for InterceptOptions {
    fn default() -> Self {
        Self {
            patterns: vec![json!({
                "urlPattern": "*",
                "resourceType": "Script",
                "requestStage": "Response",
            })],
            body_types: vec!["Script".to_string()],
        }
    }
}

impl InterceptOptions {
    pub fn scripts_and_documents() -> Self {
        Self {
            patterns: vec![
                json!({ "urlPattern": "*", "resourceType": "Script", "requestStage": "Response" }),
                json!({ "urlPattern": "*", "resourceType": "Document", "requestStage": "Response" }),
            ],
            body_types: vec!["Script".to_string(), "Document".to_string()],
        }
    }

    pub fn everything() -> Self {
        Self {
            patterns: vec![json!({ "urlPattern": "*", "requestStage": "Response" })],
            body_types: vec![
                "Script".to_string(),
                "Document".to_string(),
                "XHR".to_string(),
                "Fetch".to_string(),
                "Stylesheet".to_string(),
            ],
        }
    }
}

pub async fn start(
    session: &Session,
    options: InterceptOptions,
    handler: Handler,
) -> Result<JoinHandle<()>> {
    session
        .send("Fetch.enable", json!({ "patterns": options.patterns }))
        .await?;

    let mut events = session.events().await;
    let pump_session = session.clone();
    let body_types = options.body_types.clone();

    let task = tokio::spawn(async move {
        while let Some(event) = events.recv().await {
            if event.method != "Fetch.requestPaused" {
                continue;
            }

            let params = event.params;
            let Some(request_id) = params.get("requestId").and_then(Value::as_str) else {
                continue;
            };
            let request_id = request_id.to_string();

            let resource_type = params
                .get("resourceType")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string();

            let status = params
                .get("responseStatusCode")
                .and_then(Value::as_u64)
                .map(|value| value as u16);

            let mut paused = PausedRequest {
                request_id: request_id.clone(),
                url: params
                    .get("request")
                    .and_then(|value| value.get("url"))
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_string(),
                method: params
                    .get("request")
                    .and_then(|value| value.get("method"))
                    .and_then(Value::as_str)
                    .unwrap_or("GET")
                    .to_string(),
                resource_type: resource_type.clone(),
                status,
                request_headers: header_pairs(
                    params.get("request").and_then(|value| value.get("headers")),
                ),
                response_headers: response_header_pairs(params.get("responseHeaders")),
                post_data: params
                    .get("request")
                    .and_then(|value| value.get("postData"))
                    .and_then(Value::as_str)
                    .map(str::to_string),
                body: None,
            };

            if status.is_some() && body_types.contains(&resource_type) {
                if let Ok(result) = pump_session
                    .send("Fetch.getResponseBody", json!({ "requestId": request_id }))
                    .await
                {
                    paused.body = decode_body(&result).ok();
                }
            }

            let decision = handler(&paused);
            apply(&pump_session, &request_id, decision).await;
        }
    });

    Ok(task)
}

async fn apply(session: &Session, request_id: &str, decision: Decision) {
    match decision {
        Decision::Continue => {
            session
                .try_send("Fetch.continueRequest", json!({ "requestId": request_id }))
                .await;
        }
        Decision::Fail(reason) => {
            session
                .try_send(
                    "Fetch.failRequest",
                    json!({ "requestId": request_id, "errorReason": reason }),
                )
                .await;
        }
        Decision::Fulfill { status, headers, body } => {
            let headers: Vec<Value> = headers
                .into_iter()
                .map(|(name, value)| json!({ "name": name, "value": value }))
                .collect();

            let sent = session
                .try_send(
                    "Fetch.fulfillRequest",
                    json!({
                        "requestId": request_id,
                        "responseCode": status,
                        "responseHeaders": headers,
                        "body": STANDARD.encode(&body),
                    }),
                )
                .await;

            if sent.is_none() {
                session
                    .try_send("Fetch.continueRequest", json!({ "requestId": request_id }))
                    .await;
            }
        }
    }
}

fn header_pairs(value: Option<&Value>) -> Vec<(String, String)> {
    let Some(Value::Object(map)) = value else {
        return Vec::new();
    };

    map.iter()
        .map(|(name, value)| {
            (
                name.clone(),
                value.as_str().unwrap_or_default().to_string(),
            )
        })
        .collect()
}

fn response_header_pairs(value: Option<&Value>) -> Vec<(String, String)> {
    let Some(Value::Array(items)) = value else {
        return Vec::new();
    };

    items
        .iter()
        .filter_map(|entry| {
            Some((
                entry.get("name")?.as_str()?.to_string(),
                entry.get("value")?.as_str().unwrap_or_default().to_string(),
            ))
        })
        .collect()
}

pub fn rewrite_scripts<F>(rewrite: F) -> Handler
where
    F: Fn(&str, &str) -> Option<String> + Send + Sync + 'static,
{
    Arc::new(move |request: &PausedRequest| {
        if !request.is_response_stage() {
            return Decision::Continue;
        }

        let Some(source) = request.text() else {
            return Decision::Continue;
        };

        match rewrite(&request.url, &source) {
            Some(replacement) => Decision::serve(request, replacement.into_bytes()),
            None => Decision::Continue,
        }
    })
}
