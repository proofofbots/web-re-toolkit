use std::io::{Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::sync::mpsc;
use std::sync::Arc;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use serde_json::json;

use wre_core::error::{Error, Result, io};

use crate::library::Origin;
use crate::profile::Profile;

pub const PAGE: &str = include_str!("../assets/capture.html");

const HEAD_LIMIT: usize = 64 * 1024;
const BODY_LIMIT: usize = 16 * 1024 * 1024;

#[derive(Debug, Clone, Deserialize)]
pub struct Incoming {
    #[serde(default)]
    pub label: String,
    #[serde(default)]
    pub notes: String,
    #[serde(default)]
    pub origin: Origin,
    pub profile: Profile,
}

#[derive(Debug, Clone, Serialize)]
pub struct Stored {
    pub id: String,
    pub path: String,
    pub warnings: usize,
}

pub struct Server {
    listener: TcpListener,
    address: SocketAddr,
    auto: Option<String>,
}

impl Server {
    pub fn bind(host: &str, port: u16) -> Result<Self> {
        let listener = TcpListener::bind((host, port))
            .map_err(|error| Error::msg(format!("cannot listen on {host}:{port}: {error}")))?;
        let address = listener.local_addr().map_err(io("listener"))?;

        Ok(Self { listener, address, auto: None })
    }

    pub fn sending_on_its_own(mut self, label: Option<String>) -> Self {
        self.auto = Some(label.unwrap_or_default());
        self
    }

    pub fn page(&self) -> String {
        let Some(label) = &self.auto else {
            return PAGE.to_string();
        };

        let marker = format!(
            "<script>window.__wreCapture = {{ auto: true, label: {} }};</script>\n</head>",
            json!(label)
        );

        PAGE.replacen("</head>", &marker, 1)
    }

    pub fn address(&self) -> SocketAddr {
        self.address
    }

    pub fn url(&self) -> String {
        let address = self.address;
        if address.ip().is_unspecified() {
            format!("http://127.0.0.1:{}", address.port())
        } else {
            format!("http://{address}")
        }
    }

    pub fn run<F>(&self, keep: bool, mut store: F) -> Result<usize>
    where
        F: FnMut(Incoming, &str) -> Result<Stored>,
    {
        let listener = self.listener.try_clone().map_err(io("listener"))?;
        let (jobs, inbox) = mpsc::channel::<Job>();
        let page = Arc::new(self.page());

        std::thread::Builder::new()
            .name("wre-capture-accept".to_string())
            .spawn(move || {
                for stream in listener.incoming() {
                    let Ok(mut stream) = stream else { continue };
                    let jobs = jobs.clone();
                    let page = Arc::clone(&page);

                    let spawned = std::thread::Builder::new()
                        .name("wre-capture-connection".to_string())
                        .spawn(move || {
                            let peer = stream
                                .peer_addr()
                                .map(|address| address.ip().to_string())
                                .unwrap_or_else(|_| "unknown".to_string());

                            let _ = stream.set_read_timeout(Some(Duration::from_secs(20)));
                            let _ = stream.set_write_timeout(Some(Duration::from_secs(20)));

                            if let Err(error) = handle(&mut stream, &peer, &jobs, &page) {
                                tracing::debug!("sandbox capture request failed: {error}");
                            }
                        });

                    if spawned.is_err() {
                        tracing::warn!("sandbox capture could not start a connection thread");
                    }
                }
            })
            .map_err(io("listener"))?;

        let mut taken = 0usize;

        while let Ok(job) = inbox.recv() {
            let outcome = store(job.incoming, &job.peer);
            let answered = match &outcome {
                Ok(stored) => job.reply.send(Ok(stored.clone())),
                Err(error) => job.reply.send(Err(error.to_string())),
            };

            if answered.is_err() {
                tracing::debug!("the capture client left before the answer was written");
            }

            if outcome.is_ok() {
                taken += 1;
                if !keep {
                    return Ok(taken);
                }
            }
        }

        Ok(taken)
    }
}

struct Job {
    incoming: Incoming,
    peer: String,
    reply: mpsc::Sender<std::result::Result<Stored, String>>,
}

fn handle(
    stream: &mut TcpStream,
    peer: &str,
    jobs: &mpsc::Sender<Job>,
    page: &str,
) -> Result<bool> {
    let (head, rest) = read_head(stream)?;
    let mut lines = head.split("\r\n");
    let request = lines.next().unwrap_or_default();
    let mut parts = request.split(' ');
    let method = parts.next().unwrap_or_default();
    let path = parts.next().unwrap_or_default();

    match (method, path) {
        ("GET", "/") | ("GET", "/index.html") => {
            respond(stream, 200, "text/html; charset=utf-8", page.as_bytes())?;
            Ok(false)
        }

        ("POST", "/profile") => {
            let length = content_length(&head)?;
            let body = read_body(stream, rest, length)?;

            let incoming: Incoming = match serde_json::from_slice(&body) {
                Ok(incoming) => incoming,
                Err(error) => {
                    let message = json!({ "error": format!("the capture did not parse: {error}") });
                    respond_json(stream, 400, &message)?;
                    return Ok(false);
                }
            };

            let (reply, answer) = mpsc::channel();

            if jobs
                .send(Job { incoming, peer: peer.to_string(), reply })
                .is_err()
            {
                let message = json!({ "error": "the capture host stopped listening" });
                respond_json(stream, 503, &message)?;
                return Ok(false);
            }

            match answer.recv() {
                Ok(Ok(stored)) => {
                    respond_json(stream, 200, &json!(stored))?;
                    Ok(true)
                }
                Ok(Err(error)) => {
                    respond_json(stream, 500, &json!({ "error": error }))?;
                    Ok(false)
                }
                Err(_) => {
                    respond_json(stream, 500, &json!({ "error": "the capture host went away" }))?;
                    Ok(false)
                }
            }
        }

        ("GET", "/favicon.ico") => {
            respond(stream, 404, "text/plain; charset=utf-8", b"")?;
            Ok(false)
        }

        _ => {
            respond(stream, 404, "text/plain; charset=utf-8", b"not here\n")?;
            Ok(false)
        }
    }
}

fn read_head(stream: &mut TcpStream) -> Result<(String, Vec<u8>)> {
    let mut buffer = Vec::new();
    let mut chunk = [0u8; 4096];

    loop {
        let read = stream.read(&mut chunk).map_err(io("capture request"))?;
        if read == 0 {
            return Err(Error::msg("the client closed before sending a request"));
        }

        buffer.extend_from_slice(&chunk[..read]);

        if let Some(end) = find(&buffer, b"\r\n\r\n") {
            let head = String::from_utf8_lossy(&buffer[..end]).to_string();
            let rest = buffer[end + 4..].to_vec();
            return Ok((head, rest));
        }

        if buffer.len() > HEAD_LIMIT {
            return Err(Error::msg("the request head is too long"));
        }
    }
}

fn read_body(stream: &mut TcpStream, mut body: Vec<u8>, length: usize) -> Result<Vec<u8>> {
    if length > BODY_LIMIT {
        return Err(Error::msg(format!("the capture is {length} bytes, over the limit")));
    }

    let mut chunk = [0u8; 8192];
    while body.len() < length {
        let read = stream.read(&mut chunk).map_err(io("capture body"))?;
        if read == 0 {
            return Err(Error::msg("the client closed mid body"));
        }
        body.extend_from_slice(&chunk[..read]);
    }

    body.truncate(length);
    Ok(body)
}

fn content_length(head: &str) -> Result<usize> {
    for line in head.split("\r\n").skip(1) {
        let Some((name, value)) = line.split_once(':') else { continue };
        if name.trim().eq_ignore_ascii_case("content-length") {
            return value
                .trim()
                .parse()
                .map_err(|_| Error::msg(format!("bad content-length: {value}")));
        }
    }

    Err(Error::msg("the capture arrived without a content-length"))
}

fn find(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack.windows(needle.len()).position(|window| window == needle)
}

fn reason(status: u16) -> &'static str {
    match status {
        200 => "OK",
        400 => "Bad Request",
        404 => "Not Found",
        _ => "Internal Server Error",
    }
}

fn respond(stream: &mut TcpStream, status: u16, kind: &str, body: &[u8]) -> Result<()> {
    let head = format!(
        "HTTP/1.1 {status} {}\r\ncontent-type: {kind}\r\ncontent-length: {}\r\n\
         cache-control: no-store\r\nconnection: close\r\n\r\n",
        reason(status),
        body.len()
    );

    stream.write_all(head.as_bytes()).map_err(io("capture response"))?;
    stream.write_all(body).map_err(io("capture response"))?;
    stream.flush().map_err(io("capture response"))?;
    Ok(())
}

fn respond_json(stream: &mut TcpStream, status: u16, value: &serde_json::Value) -> Result<()> {
    let body = serde_json::to_vec(value)
        .map_err(|error| Error::msg(format!("response did not serialise: {error}")))?;
    respond(stream, status, "application/json", &body)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_page_serves_and_a_capture_is_handed_to_the_sink() {
        let server = Server::bind("127.0.0.1", 0).unwrap();
        let port = server.address().port();

        let worker = std::thread::spawn(move || {
            server
                .run(false, |incoming, peer| {
                    assert_eq!(peer, "127.0.0.1");
                    Ok(Stored {
                        id: incoming.label.clone(),
                        path: "/tmp/x.json".to_string(),
                        warnings: 0,
                    })
                })
                .unwrap()
        });

        let mut page = TcpStream::connect(("127.0.0.1", port)).unwrap();
        page.write_all(b"GET / HTTP/1.1\r\nhost: localhost\r\n\r\n").unwrap();
        let mut text = String::new();
        page.read_to_string(&mut text).unwrap();
        assert!(text.contains("wre sandbox capture"), "{text}");

        let profile = serde_json::to_string(&json!({
            "label": "test-device",
            "profile": Profile::desktop_chrome(),
        }))
        .unwrap();

        let mut post = TcpStream::connect(("127.0.0.1", port)).unwrap();
        post.write_all(
            format!(
                "POST /profile HTTP/1.1\r\nhost: localhost\r\ncontent-type: application/json\r\n\
                 content-length: {}\r\n\r\n{profile}",
                profile.len()
            )
            .as_bytes(),
        )
        .unwrap();

        let mut answer = String::new();
        post.read_to_string(&mut answer).unwrap();
        assert!(answer.contains("test-device"), "{answer}");

        assert_eq!(worker.join().unwrap(), 1);
    }

    #[test]
    fn a_body_that_is_not_a_profile_is_rejected_without_taking_a_capture() {
        let server = Server::bind("127.0.0.1", 0).unwrap();
        let port = server.address().port();

        std::thread::spawn(move || {
            let _ = server.run(true, |_incoming, _peer| {
                panic!("a body that is not a profile must never reach the sink");
            });
        });

        let mut post = TcpStream::connect(("127.0.0.1", port)).unwrap();
        post.write_all(b"POST /profile HTTP/1.1\r\ncontent-length: 2\r\n\r\n{}").unwrap();

        let mut answer = String::new();
        post.read_to_string(&mut answer).unwrap();
        assert!(answer.starts_with("HTTP/1.1 400"), "{answer}");
        assert!(answer.contains("did not parse"), "{answer}");
    }
}
