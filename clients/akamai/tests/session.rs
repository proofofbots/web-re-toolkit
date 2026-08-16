use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;

use serde_json::{Value, json};

use wre_client::client::{Client, prepare, prepare_params};
use wre_client::context::{Ctx, Services};

const SENSOR: &str = r##"
(function () {
  var started = Date.now();
  var seen = { moves: 0, keys: 0, clicks: 0 };

  var facts = [];

  function note(name, value) {
    facts.push(name + "=" + String(value));
  }

  note("ua", navigator.userAgent);
  note("plat", navigator.platform);
  note("wd", navigator.webdriver);
  note("cores", navigator.hardwareConcurrency);
  note("mem", navigator.deviceMemory);
  note("touch", navigator.maxTouchPoints);
  note("lang", navigator.languages.join(","));
  note("plugins", navigator.plugins.length);
  note("named", navigator.plugins["Chrome PDF Viewer"] ? navigator.plugins["Chrome PDF Viewer"].name : "none");
  note("mime", navigator.mimeTypes.length);
  note("screen", screen.width + "x" + screen.height + "x" + screen.colorDepth);
  note("win", innerWidth + "x" + innerHeight + "@" + devicePixelRatio);
  note("tz", Intl.DateTimeFormat().resolvedOptions().timeZone);
  note("off", new Date().getTimezoneOffset());
  note("mq", matchMedia("(pointer: fine)").matches);
  note("battery", typeof navigator.getBattery);
  note("uad", navigator.userAgentData ? navigator.userAgentData.platform : "none");
  note("rtt", navigator.connection ? navigator.connection.rtt : -1);
  note("storage", (function () {
    try {
      localStorage.setItem("dummy", "test");
      return localStorage.getItem("dummy") === "test";
    } catch (error) {
      return false;
    }
  })());
  note("eem", [typeof DeviceOrientationEvent, typeof DeviceMotionEvent, typeof TouchEvent].join("/"));
  note("x12", "FileReader" in window);
  note("xag", typeof PointerEvent);
  note("push", typeof PushManager);
  note("xpath", typeof XPathResult);
  note("pt", Document.prototype.hasOwnProperty("hasPrivateToken"));
  note("inputs", document.getElementsByTagName("input").length);
  note("forms", document.forms.length);
  note("title", document.title);
  note("href", location.href);
  note("scripts", document.scripts.length);
  note("current", document.currentScript ? document.currentScript.src.slice(-8) : "none");
  note("native", document.createElement.toString().indexOf("[native code]") >= 0);

  var canvas = document.createElement("canvas");
  canvas.width = 240;
  canvas.height = 60;
  var context = canvas.getContext("2d");
  context.textBaseline = "top";
  context.font = "14px 'Arial'";
  context.fillStyle = "#f60";
  context.fillRect(125, 1, 62, 20);
  context.fillStyle = "#069";
  context.fillText("sensor probe", 2, 15);
  note("canvas", canvas.toDataURL().length);
  note("width", Math.round(context.measureText("mmmmmmmmmmlli").width * 100) / 100);

  var gl = document.createElement("canvas").getContext("webgl");
  var info = gl.getExtension("WEBGL_debug_renderer_info");
  note("gpu", gl.getParameter(info.UNMASKED_RENDERER_WEBGL));
  note("glext", gl.getSupportedExtensions().length);

  document.addEventListener("mousemove", function () { seen.moves += 1; });
  document.addEventListener("keydown", function () { seen.keys += 1; });
  document.addEventListener("click", function () { seen.clicks += 1; });
  addEventListener("load", function () { note("load", Date.now() - started); });

  function payload() {
    return [
      "7a74G7m23Vrp0o5c9at4",
      String(bmak.startTs),
      String(Date.now() - bmak.startTs),
      "moves:" + seen.moves,
      "keys:" + seen.keys,
      "clicks:" + seen.clicks,
      "cookie:" + (document.cookie.indexOf("_abck") >= 0),
      facts.join("|")
    ].join(",");
  }

  var bmak = {
    startTs: 0,
    get_telemetry: function () {
      return "a=1&&&b=2&&&sensor_data=" + btoa(payload());
    }
  };

  globalThis.bmak = bmak;
  bmak.startTs = Date.now();

  setTimeout(function () {
    var request = new XMLHttpRequest();
    request.open("POST", document.currentScript ? document.currentScript.src : location.href);
    request.setRequestHeader("Content-Type", "application/json");
    request.send(JSON.stringify({ sensor_data: payload() }));
    globalThis.posted = request.status;
  }, 1200);
})();
"##;

const SENSOR_PATH: &str = "/pWSY7c1/2ib/AKfr/hFDsQoTHmWt/YwEfMkyRK8Um/Y3g/AWJXAjJfBQ";

fn page(origin: &str) -> String {
    format!(
        r#"<!doctype html><html><head><title>Sign in</title>
<script type="text/javascript">bazadebezolkohpepadr="1320943881"</script>
<script type="text/javascript" src="{origin}/akam/13/4ebc0144"></script>
<script type="text/javascript" src="{origin}/akam/13/pixel_4ebc0144?a=dD0xJmpzPW9mZg=="></script>
<script src="{origin}{SENSOR_PATH}"></script>
</head><body>
<form method="post" action="/identity/user/login" id="login">
<input type="hidden" name="__RequestVerificationToken" value="token-value">
<input type="email" name="Username" required>
<input type="password" name="Password">
</form></body></html>"#
    )
}

struct Edge {
    port: u16,
    posts: Arc<Mutex<Vec<String>>>,
    telemetry: Arc<Mutex<Vec<String>>>,
    served: Arc<AtomicUsize>,
}

fn serve() -> Edge {
    let listener = TcpListener::bind("127.0.0.1:0").expect("listener");
    let port = listener.local_addr().expect("address").port();

    let posts = Arc::new(Mutex::new(Vec::new()));
    let telemetry = Arc::new(Mutex::new(Vec::new()));
    let served = Arc::new(AtomicUsize::new(0));

    let edge = Edge {
        port,
        posts: Arc::clone(&posts),
        telemetry: Arc::clone(&telemetry),
        served: Arc::clone(&served),
    };

    thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(stream) = stream else { continue };
            let posts = Arc::clone(&posts);
            let telemetry = Arc::clone(&telemetry);
            let served = Arc::clone(&served);
            thread::spawn(move || answer(stream, port, posts, telemetry, served));
        }
    });

    edge
}

fn answer(
    mut stream: TcpStream,
    port: u16,
    posts: Arc<Mutex<Vec<String>>>,
    telemetry: Arc<Mutex<Vec<String>>>,
    served: Arc<AtomicUsize>,
) {
    let peek = stream.try_clone().expect("clone");
    let mut reader = BufReader::new(peek);
    let mut request = String::new();

    if reader.read_line(&mut request).is_err() {
        return;
    }

    let mut headers = Vec::new();
    loop {
        let mut line = String::new();
        if reader.read_line(&mut line).unwrap_or(0) == 0 {
            break;
        }
        if line.trim().is_empty() {
            break;
        }
        headers.push(line.trim().to_string());
    }

    let length = headers
        .iter()
        .find(|line| line.to_lowercase().starts_with("content-length:"))
        .and_then(|line| line.split(':').nth(1))
        .and_then(|value| value.trim().parse::<usize>().ok())
        .unwrap_or(0);

    let mut body = vec![0u8; length];
    if length > 0 {
        let _ = reader.read_exact(&mut body);
    }

    let body = String::from_utf8_lossy(&body).into_owned();
    let mut parts = request.split_whitespace();
    let method = parts.next().unwrap_or("GET").to_string();
    let path = parts.next().unwrap_or("/").to_string();

    let header = |name: &str| -> String {
        headers
            .iter()
            .find(|line| line.to_lowercase().starts_with(&format!("{name}:")))
            .and_then(|line| line.split_once(':'))
            .map(|(_, value)| value.trim().to_string())
            .unwrap_or_default()
    };

    let cookie = header("cookie");

    let reply = |stream: &mut TcpStream, status: &str, kind: &str, extra: &[String], body: &str| {
        let mut head = format!(
            "HTTP/1.1 {status}\r\ncontent-type: {kind}\r\ncontent-length: {}\r\nconnection: close\r\n",
            body.len()
        );
        for line in extra {
            head.push_str(line);
            head.push_str("\r\n");
        }
        head.push_str("\r\n");
        let _ = stream.write_all(head.as_bytes());
        let _ = stream.write_all(body.as_bytes());
        let _ = stream.flush();
    };

    match (method.as_str(), path.as_str()) {
        ("GET", "/login") => {
            let cookies = vec![
                "set-cookie: _abck=E7A1~-1~seed~-1~-1; Path=/".to_string(),
                "set-cookie: bm_sz=AB12~YAAQ~3; Path=/".to_string(),
                "set-cookie: ak_bmsc=secret; Path=/; HttpOnly".to_string(),
            ];
            reply(
                &mut stream,
                "200 OK",
                "text/html; charset=utf-8",
                &cookies,
                &page(&format!("http://127.0.0.1:{port}")),
            );
        }

        ("GET", path) if path == SENSOR_PATH => {
            reply(&mut stream, "200 OK", "application/javascript", &[], SENSOR);
        }

        ("POST", path) if path == SENSOR_PATH => {
            posts.lock().unwrap().push(body.clone());
            let cookies = vec!["set-cookie: _abck=E7A1~0~valid~-1~-1; Path=/".to_string()];
            reply(&mut stream, "201 Created", "application/json", &cookies, r#"{"success":true}"#);
        }

        ("GET", "/akam/13/4ebc0144") => {
            let script = "(function () { var request = new XMLHttpRequest(); \
                 request.open('POST', location.origin + '/akam/13/pixel_' + (77 ^ Number(bazadebezolkohpepadr)).toString(16)); \
                 request.setRequestHeader('Content-Type', 'application/x-www-form-urlencoded'); \
                 request.send('ap=true&br=Chrome&cv=' + encodeURIComponent(document.createElement('canvas').toDataURL().length)); })();";
            reply(&mut stream, "200 OK", "application/javascript", &[], script);
        }

        ("POST", path) if path.starts_with("/akam/13/pixel_") => {
            posts.lock().unwrap().push(format!("pixel:{body}"));
            reply(&mut stream, "200 OK", "text/plain", &[], "");
        }

        (_, "/api/orders") => {
            let sent = header("akamai-bm-telemetry");
            let validated = cookie.contains("_abck=E7A1~0~");

            if !sent.is_empty() {
                telemetry.lock().unwrap().push(sent.clone());
            }

            if validated && !sent.is_empty() {
                served.fetch_add(1, Ordering::Relaxed);
                reply(&mut stream, "200 OK", "application/json", &[], r#"{"orders":[]}"#);
            } else {
                reply(
                    &mut stream,
                    "403 Forbidden",
                    "text/html",
                    &[],
                    "<html><body>Access Denied</body></html>",
                );
            }
        }

        _ => reply(&mut stream, "404 Not Found", "text/plain", &[], "no"),
    }
}

fn client(config: Value) -> (Box<dyn Client>, wre_client::spec::ClientDescriptor) {
    let services = Services::detached().expect("services");
    let ctx = Ctx::new("akamai", "test", services);
    let descriptor = wre_client_akamai::describe().seal().expect("descriptor");
    let config = prepare(&descriptor, &descriptor.config, config, "config").expect("config");
    let client = (wre_client_akamai::registration().build)(ctx, config).expect("client");

    (client, descriptor)
}

fn call(
    client: &mut Box<dyn Client>,
    descriptor: &wre_client::spec::ClientDescriptor,
    op: &str,
    params: Value,
) -> Value {
    let params = prepare_params(descriptor, op, params).expect("params");
    let call = wre_client::context::Call::detached(op);

    client.call(op, params, &call).unwrap_or_else(|error| panic!("{op} failed: {error}"))
}

#[test]
fn the_client_runs_a_sensor_and_carries_the_session_it_produces() {
    let edge = serve();
    let url = format!("http://127.0.0.1:{}/login", edge.port);

    let (mut client, descriptor) = client(json!({
        "page_url": url,
        "wait_ms": 4000,
        "rounds": 2,
        "seed": 42,
    }));

    let found = call(&mut client, &descriptor, "discover", json!({}));
    assert_eq!(found["status"], 200);
    assert_eq!(found["protected"], true);
    assert_eq!(found["surface"]["sensor"]["kind"], "obfuscated");
    assert_eq!(found["surface"]["baza"], "1320943881");
    assert!(
        found["surface"]["pixel_post"]
            .as_str()
            .unwrap()
            .ends_with("/akam/13/pixel_4ebc0144")
    );
    assert_eq!(found["cookies"]["state"], "unvalidated");

    let solved = call(&mut client, &descriptor, "solve", json!({}));

    let payload = solved["payload"].as_str().expect("payload");
    assert!(payload.starts_with("7a74G7m23Vrp0o5c9at4"), "{payload}");
    assert!(payload.contains("cookie:true"), "{payload}");
    assert!(payload.contains("wd=false"), "{payload}");
    assert!(payload.contains("native=true"), "{payload}");
    assert!(payload.contains("pt=true"), "{payload}");
    assert!(payload.contains("gpu=ANGLE (Apple"), "{payload}");
    assert!(payload.contains("inputs=3"), "{payload}");
    assert!(payload.contains("tz=Europe/London"), "{payload}");

    let moves = payload
        .split("moves:")
        .nth(1)
        .and_then(|rest| rest.split(',').next())
        .and_then(|value| value.parse::<u32>().ok())
        .expect("moves");
    assert!(moves > 5, "the behaviour stream produced {moves} moves");

    assert!(payload.contains("keys:2"), "{payload}");
    assert!(payload.contains("clicks:1"), "{payload}");

    let telemetry = solved["telemetry"].as_str().expect("telemetry");
    assert!(telemetry.contains("sensor_data="));
    assert_eq!(solved["run"]["threw"], Value::Null);
    assert!(solved["run"]["misses"].as_array().unwrap().len() < 8, "{}", solved["run"]["misses"]);

    assert_eq!(solved["cookies"]["state"], "validated");
    assert!(solved["posts"].as_array().unwrap().len() >= 2, "{}", solved["posts"]);

    let posts = edge.posts.lock().unwrap().clone();
    assert!(posts.len() >= 2, "{posts:?}");
    assert!(posts.iter().any(|body| body.contains("sensor_data")));
    assert!(posts.iter().any(|body| body.starts_with("pixel:")), "the pixel client did not post");

    let answered = call(
        &mut client,
        &descriptor,
        "request",
        json!({ "url": format!("http://127.0.0.1:{}/api/orders", edge.port), "telemetry": true }),
    );

    assert_eq!(answered["status"], 200, "{}", answered["body"]);
    assert_eq!(answered["refused"], false);
    assert_eq!(edge.served.load(Ordering::Relaxed), 1);
    assert_eq!(edge.telemetry.lock().unwrap().len(), 1);
}

#[test]
fn a_request_without_the_session_is_refused_by_the_edge() {
    let edge = serve();
    let url = format!("http://127.0.0.1:{}/login", edge.port);

    let (mut client, descriptor) = client(json!({ "page_url": url, "wait_ms": 1000 }));

    let answered = call(
        &mut client,
        &descriptor,
        "request",
        json!({ "url": format!("http://127.0.0.1:{}/api/orders", edge.port) }),
    );

    assert_eq!(answered["status"], 403);
    assert_eq!(answered["refused"], true);
    assert_eq!(edge.served.load(Ordering::Relaxed), 0);
}

#[test]
fn a_fresh_payload_can_be_built_and_posted_on_demand() {
    let edge = serve();
    let url = format!("http://127.0.0.1:{}/login", edge.port);

    let (mut client, descriptor) = client(json!({
        "page_url": url,
        "wait_ms": 2000,
        "rounds": 1,
        "pixel": false,
    }));

    call(&mut client, &descriptor, "solve", json!({}));

    let first = call(&mut client, &descriptor, "payload", json!({}));
    let second = call(&mut client, &descriptor, "payload", json!({ "nudge_ms": 2000 }));

    let first = first["payload"].as_str().expect("first payload");
    let second = second["payload"].as_str().expect("second payload");

    assert_ne!(first, second, "two payloads from one session should not be identical");

    let posted = call(&mut client, &descriptor, "post", json!({ "rounds": 1 }));
    assert_eq!(posted["posts"][0]["status"], 201);

    let cookies = call(&mut client, &descriptor, "cookies", json!({}));
    assert!(cookies["header"].as_str().unwrap().contains("_abck="));
    assert_eq!(cookies["summary"]["state"], "validated");

    let closed = call(&mut client, &descriptor, "reset", json!({ "cookies": true }));
    assert_eq!(closed["open"], false);

    let names = call(&mut client, &descriptor, "cookies", json!({}));
    assert_eq!(names["names"].as_array().unwrap().len(), 0);
}

#[test]
fn the_proof_of_work_op_answers_a_challenge_carried_in_a_cookie() {
    let (mut client, descriptor) = client(json!({}));

    let answer = call(
        &mut client,
        &descriptor,
        "pow",
        json!({
            "abck": "TOKEN~-1~salt~-1~3-abcd-500-1000-30-2",
            "start_ts": 1_760_000_000_000u64,
            "rounds": 3,
        }),
    );

    assert_eq!(answer["challenge"]["salt"], "abcd");
    assert_eq!(answer["nonces"].as_array().unwrap().len(), 3);
    assert_eq!(answer["prefix"], "TOKEN1760000000000abcd");
    assert!(answer["answer"].as_str().unwrap().ends_with(';'));
}

#[test]
fn the_client_reports_what_it_is_carrying() {
    let (mut client, descriptor) = client(json!({}));

    let info = call(&mut client, &descriptor, "info", json!({}));

    assert_eq!(info["target"], "akamai");
    assert_eq!(info["profile"], "builtin-desktop-chrome");
    assert!(info["user_agent"].as_str().unwrap().contains("Chrome/140"));
    assert_eq!(info["open"], false);
}
