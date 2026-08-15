use std::collections::BTreeMap;
use std::path::Path;

use serde_json::Value;
use tokio::sync::mpsc;

use wre_cdp::connection::Event;
use wre_cdp::session::{Session, decode_body};
use wre_core::bundle::{BodyRef, ConsoleRecord, ExceptionRecord, RequestRecord};
use wre_core::error::Result;
use wre_core::paths::safe_name;

#[derive(Debug, Default)]
pub struct Collected {
    pub requests: BTreeMap<String, RequestRecord>,
    pub order: Vec<String>,
    pub console: Vec<ConsoleRecord>,
    pub exceptions: Vec<ExceptionRecord>,
    pub set_cookies: Vec<String>,
}

impl Collected {
    pub fn absorb(&mut self, event: &Event) {
        match event.method.as_str() {
            "Network.requestWillBeSent" => self.request_started(&event.params),
            "Network.requestWillBeSentExtraInfo" => self.request_cookies(&event.params),
            "Network.responseReceived" => self.response(&event.params),
            "Network.responseReceivedExtraInfo" => self.response_cookies(&event.params),
            "Network.loadingFailed" => self.failed(&event.params),
            "Runtime.consoleAPICalled" => self.console_line(&event.params),
            "Runtime.exceptionThrown" => self.exception(&event.params),
            _ => {}
        }
    }

    fn entry(&mut self, id: &str) -> Option<&mut RequestRecord> {
        self.requests.get_mut(id)
    }

    fn request_started(&mut self, params: &Value) {
        let Some(id) = params.get("requestId").and_then(Value::as_str) else {
            return;
        };

        let request = params.get("request");

        let record = RequestRecord {
            id: id.to_string(),
            url: request
                .and_then(|value| value.get("url"))
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string(),
            method: request
                .and_then(|value| value.get("method"))
                .and_then(Value::as_str)
                .unwrap_or("GET")
                .to_string(),
            resource_type: params
                .get("type")
                .and_then(Value::as_str)
                .map(str::to_string),
            request_headers: header_map(request.and_then(|value| value.get("headers"))),
            request_body: match request
                .and_then(|value| value.get("postData"))
                .and_then(Value::as_str)
            {
                Some(text) => BodyRef {
                    inline: Some(text.to_string()),
                    size: text.len(),
                    ..BodyRef::default()
                },
                None => BodyRef::empty(),
            },
            status: None,
            response_headers: BTreeMap::new(),
            response_body: BodyRef::empty(),
            remote_address: None,
            protocol: None,
            initiator: params
                .get("initiator")
                .and_then(|value| value.get("type"))
                .and_then(Value::as_str)
                .map(str::to_string),
            initiator_stack: params
                .get("initiator")
                .and_then(|value| value.get("stack"))
                .and_then(|value| value.get("callFrames"))
                .and_then(Value::as_array)
                .map(|frames| {
                    frames
                        .iter()
                        .take(4)
                        .map(|frame| {
                            format!(
                                "{}:{}",
                                frame.get("url").and_then(Value::as_str).unwrap_or_default(),
                                frame
                                    .get("lineNumber")
                                    .and_then(Value::as_u64)
                                    .unwrap_or_default()
                            )
                        })
                        .collect()
                })
                .unwrap_or_default(),
            sent_cookies: Vec::new(),
            set_cookies: Vec::new(),
            wall_time: params.get("wallTime").and_then(Value::as_f64),
            error: None,
        };

        if !self.requests.contains_key(id) {
            self.order.push(id.to_string());
        }

        self.requests.insert(id.to_string(), record);
    }

    fn request_cookies(&mut self, params: &Value) {
        let Some(id) = params.get("requestId").and_then(Value::as_str) else {
            return;
        };
        let names: Vec<String> = params
            .get("associatedCookies")
            .and_then(Value::as_array)
            .map(|items| {
                items
                    .iter()
                    .filter_map(|item| {
                        item.get("cookie")
                            .and_then(|cookie| cookie.get("name"))
                            .and_then(Value::as_str)
                            .map(str::to_string)
                    })
                    .collect()
            })
            .unwrap_or_default();

        let id = id.to_string();
        if let Some(entry) = self.entry(&id) {
            entry.sent_cookies = names;
        }
    }

    fn response(&mut self, params: &Value) {
        let Some(id) = params.get("requestId").and_then(Value::as_str) else {
            return;
        };
        let response = params.get("response");
        let id = id.to_string();

        let status = response
            .and_then(|value| value.get("status"))
            .and_then(Value::as_u64)
            .map(|value| value as u16);
        let headers = header_map(response.and_then(|value| value.get("headers")));
        let remote = response
            .and_then(|value| value.get("remoteIPAddress"))
            .and_then(Value::as_str)
            .map(str::to_string);
        let protocol = response
            .and_then(|value| value.get("protocol"))
            .and_then(Value::as_str)
            .map(str::to_string);

        if let Some(entry) = self.entry(&id) {
            entry.status = status;
            entry.response_headers = headers;
            entry.remote_address = remote;
            entry.protocol = protocol;
        }
    }

    fn response_cookies(&mut self, params: &Value) {
        let Some(id) = params.get("requestId").and_then(Value::as_str) else {
            return;
        };

        let raw = params
            .get("headers")
            .and_then(|headers| {
                headers
                    .get("set-cookie")
                    .or_else(|| headers.get("Set-Cookie"))
            })
            .and_then(Value::as_str)
            .unwrap_or_default();

        if raw.is_empty() {
            return;
        }

        let cookies: Vec<String> = raw.lines().map(str::to_string).collect();
        for cookie in &cookies {
            self.set_cookies.push(cookie.clone());
        }

        let id = id.to_string();
        if let Some(entry) = self.entry(&id) {
            entry.set_cookies = cookies;
        }
    }

    fn failed(&mut self, params: &Value) {
        let Some(id) = params.get("requestId").and_then(Value::as_str) else {
            return;
        };
        let text = params
            .get("errorText")
            .and_then(Value::as_str)
            .unwrap_or("request failed")
            .to_string();

        let id = id.to_string();
        if let Some(entry) = self.entry(&id) {
            entry.error = Some(text);
        }
    }

    fn console_line(&mut self, params: &Value) {
        let level = params
            .get("type")
            .and_then(Value::as_str)
            .unwrap_or("log")
            .to_string();

        let text = params
            .get("args")
            .and_then(Value::as_array)
            .map(|args| {
                args.iter()
                    .map(|argument| {
                        argument
                            .get("value")
                            .and_then(Value::as_str)
                            .map(str::to_string)
                            .or_else(|| {
                                argument
                                    .get("description")
                                    .and_then(Value::as_str)
                                    .map(str::to_string)
                            })
                            .unwrap_or_else(|| {
                                argument
                                    .get("type")
                                    .and_then(Value::as_str)
                                    .unwrap_or("value")
                                    .to_string()
                            })
                    })
                    .collect::<Vec<_>>()
                    .join(" ")
            })
            .unwrap_or_default();

        if self.console.len() < 4000 {
            self.console.push(ConsoleRecord {
                level,
                text: text.chars().take(2000).collect(),
                at: params.get("timestamp").and_then(Value::as_f64),
                stack: Vec::new(),
            });
        }
    }

    fn exception(&mut self, params: &Value) {
        let details = params.get("exceptionDetails");

        self.exceptions.push(ExceptionRecord {
            text: details
                .and_then(|value| value.get("text"))
                .and_then(Value::as_str)
                .unwrap_or("exception")
                .to_string(),
            url: details
                .and_then(|value| value.get("url"))
                .and_then(Value::as_str)
                .map(str::to_string),
            line: details
                .and_then(|value| value.get("lineNumber"))
                .and_then(Value::as_u64)
                .map(|value| value as u32),
            column: details
                .and_then(|value| value.get("columnNumber"))
                .and_then(Value::as_u64)
                .map(|value| value as u32),
            description: details
                .and_then(|value| value.get("exception"))
                .and_then(|value| value.get("description"))
                .and_then(Value::as_str)
                .map(|text| text.chars().take(800).collect()),
        });
    }

    pub fn finish(self) -> (Vec<RequestRecord>, Vec<ConsoleRecord>, Vec<ExceptionRecord>) {
        let mut requests = Vec::with_capacity(self.order.len());
        let mut map = self.requests;

        for id in self.order {
            if let Some(record) = map.remove(&id) {
                requests.push(record);
            }
        }

        (requests, self.console, self.exceptions)
    }
}

pub async fn drain(receiver: &mut mpsc::UnboundedReceiver<Event>, collected: &mut Collected) {
    while let Ok(event) = receiver.try_recv() {
        collected.absorb(&event);
    }
}

pub async fn fetch_bodies(
    session: &Session,
    requests: &mut [RequestRecord],
    dir: &Path,
    interesting: impl Fn(&RequestRecord) -> bool,
) -> Result<usize> {
    let mut stored = 0usize;

    for request in requests.iter_mut() {
        if !interesting(request) {
            continue;
        }

        let Ok(result) = session
            .send(
                "Network.getResponseBody",
                serde_json::json!({ "requestId": request.id }),
            )
            .await
        else {
            continue;
        };

        let Ok(bytes) = decode_body(&result) else {
            continue;
        };

        if bytes.is_empty() {
            continue;
        }

        let name = format!("{}.{}.bin", safe_name(&request.url), request.id);
        let text_like = std::str::from_utf8(&bytes).is_ok();
        request.response_body = BodyRef::store(dir, &name, &bytes, text_like)?;
        stored += 1;
    }

    Ok(stored)
}

fn header_map(value: Option<&Value>) -> BTreeMap<String, String> {
    let Some(Value::Object(map)) = value else {
        return BTreeMap::new();
    };

    map.iter()
        .map(|(name, value)| {
            (
                name.to_ascii_lowercase(),
                value.as_str().unwrap_or_default().to_string(),
            )
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn event(method: &str, params: Value) -> Event {
        Event {
            method: method.to_string(),
            params,
            session_id: None,
        }
    }

    #[test]
    fn assembles_a_request_from_its_events() {
        let mut collected = Collected::default();

        collected.absorb(&event(
            "Network.requestWillBeSent",
            json!({
                "requestId": "1",
                "type": "XHR",
                "request": {
                    "url": "https://target.test/collect",
                    "method": "POST",
                    "headers": { "Content-Type": "text/plain" },
                    "postData": "payload"
                },
                "initiator": { "type": "script", "stack": { "callFrames": [{ "url": "a.js", "lineNumber": 3 }] } },
                "wallTime": 1.5
            }),
        ));

        collected.absorb(&event(
            "Network.responseReceived",
            json!({
                "requestId": "1",
                "response": { "status": 201, "headers": { "X-Thing": "yes" }, "protocol": "h2", "remoteIPAddress": "1.2.3.4" }
            }),
        ));

        collected.absorb(&event(
            "Network.responseReceivedExtraInfo",
            json!({ "requestId": "1", "headers": { "set-cookie": "a=1\nb=2" } }),
        ));

        let (requests, _, _) = collected.finish();
        assert_eq!(requests.len(), 1);

        let request = &requests[0];
        assert_eq!(request.method, "POST");
        assert_eq!(request.status, Some(201));
        assert_eq!(request.protocol.as_deref(), Some("h2"));
        assert_eq!(request.request_headers.get("content-type").map(String::as_str), Some("text/plain"));
        assert_eq!(request.response_headers.get("x-thing").map(String::as_str), Some("yes"));
        assert_eq!(request.set_cookies, vec!["a=1".to_string(), "b=2".to_string()]);
        assert_eq!(request.initiator_stack, vec!["a.js:3".to_string()]);
        assert_eq!(request.request_body.inline.as_deref(), Some("payload"));
    }

    #[test]
    fn records_console_and_exceptions() {
        let mut collected = Collected::default();

        collected.absorb(&event(
            "Runtime.consoleAPICalled",
            json!({ "type": "warn", "args": [{ "value": "careful" }, { "description": "Object" }] }),
        ));

        collected.absorb(&event(
            "Runtime.exceptionThrown",
            json!({ "exceptionDetails": { "text": "Uncaught", "url": "a.js", "lineNumber": 7, "exception": { "description": "TypeError: x" } } }),
        ));

        let (_, console, exceptions) = collected.finish();
        assert_eq!(console[0].level, "warn");
        assert_eq!(console[0].text, "careful Object");
        assert_eq!(exceptions[0].line, Some(7));
        assert!(exceptions[0].description.as_deref().unwrap().contains("TypeError"));
    }

    #[test]
    fn keeps_request_order() {
        let mut collected = Collected::default();

        for id in ["3", "1", "2"] {
            collected.absorb(&event(
                "Network.requestWillBeSent",
                json!({ "requestId": id, "request": { "url": format!("https://x.test/{id}"), "method": "GET" } }),
            ));
        }

        let (requests, _, _) = collected.finish();
        let urls: Vec<&str> = requests.iter().map(|request| request.url.as_str()).collect();
        assert_eq!(
            urls,
            vec!["https://x.test/3", "https://x.test/1", "https://x.test/2"]
        );
    }
}
