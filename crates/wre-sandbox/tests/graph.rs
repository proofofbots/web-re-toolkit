use std::time::Duration;

use serde_json::{Value, json};

use wre_live::realm::RealmOptions;
use wre_sandbox::browser::Hooks;
use wre_sandbox::graph::{GraphPage, GraphProfile, Tables, open};

const USER_AGENT: &str = "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/151.0.0.0 Safari/537.36";

fn reference(id: usize) -> Value {
    json!({ "k": "ref", "id": id })
}

fn text(value: &str) -> Value {
    json!({ "k": "string", "v": value })
}

fn number(value: f64) -> Value {
    json!({ "k": "number", "v": value })
}

fn data(value: Value) -> Value {
    json!({ "value": value, "w": true, "e": true, "c": true })
}

fn accessor(value: Value) -> Value {
    json!({ "accessor": true, "read": value, "set": false, "e": true, "c": true })
}

fn tiny() -> GraphProfile {
    let mut window = serde_json::Map::new();

    for name in [
        "Object",
        "Function",
        "Array",
        "String",
        "Number",
        "Boolean",
        "Symbol",
        "Math",
        "JSON",
        "Date",
        "RegExp",
        "Error",
        "TypeError",
        "RangeError",
        "SyntaxError",
        "Promise",
        "Proxy",
        "Reflect",
        "Map",
        "Set",
        "WeakMap",
        "WeakSet",
        "WeakRef",
        "ArrayBuffer",
        "DataView",
        "Uint8Array",
        "Uint16Array",
        "Uint32Array",
        "Int8Array",
        "Int16Array",
        "Int32Array",
        "Float32Array",
        "Float64Array",
        "Uint8ClampedArray",
        "BigInt",
        "Intl",
        "eval",
        "parseInt",
        "parseFloat",
        "isNaN",
        "isFinite",
        "decodeURIComponent",
        "encodeURIComponent",
        "console",
        "globalThis",
        "undefined",
        "NaN",
        "Infinity",
    ] {
        window.insert(name.to_string(), data(json!({ "k": "undefined" })));
    }

    for (name, value) in [
        ("navigator", data(reference(1))),
        ("Navigator", data(reference(2))),
        ("innerWidth", data(number(1512.0))),
        ("closed", accessor(json!({ "k": "boolean", "v": false }))),
        ("crypto", data(reference(4))),
        ("Crypto", data(reference(5))),
    ] {
        window.insert(name.to_string(), value);
    }

    let objects = json!([
        {
            "type": "object",
            "props": window,
            "proto": Value::Null,
        },
        {
            "type": "object",
            "props": {
                "userAgent": accessor(text(USER_AGENT)),
                "hardwareConcurrency": accessor(number(10.0)),
            },
            "proto": reference(3),
        },
        {
            "type": "function",
            "name": "Navigator",
            "length": 0,
            "props": { "prototype": data(reference(3)) },
            "proto": Value::Null,
        },
        {
            "type": "object",
            "name": "Navigator.prototype",
            "props": { "vendor": accessor(text("Google Inc.")) },
            "proto": Value::Null,
        },
        {
            "type": "object",
            "props": {},
            "proto": reference(6),
        },
        {
            "type": "function",
            "name": "Crypto",
            "length": 0,
            "props": { "prototype": data(reference(6)) },
            "proto": Value::Null,
        },
        {
            "type": "object",
            "name": "Crypto.prototype",
            "props": {
                "randomUUID": data(json!({ "k": "ref", "id": 7 })),
                "getRandomValues": data(json!({ "k": "ref", "id": 8 })),
            },
            "proto": Value::Null,
        },
        {
            "type": "function",
            "name": "randomUUID",
            "length": 0,
            "props": {},
            "proto": Value::Null,
        },
        {
            "type": "function",
            "name": "getRandomValues",
            "length": 1,
            "props": {},
            "proto": Value::Null,
        },
    ]);

    GraphProfile {
        id: "tiny".to_string(),
        label: "a four object stand in".to_string(),
        captured_at: String::new(),
        href: "https://example.test/".to_string(),
        user_agent: USER_AGENT.to_string(),
        snapshot: json!({
            "at": "",
            "href": "https://example.test/",
            "userAgent": USER_AGENT,
            "roots": { "window": reference(0), "navigator": reference(1) },
            "objects": objects,
            "values": {},
            "systemColors": {},
            "computedStyle": {},
            "samples": {},
        }),
        tables: Tables::default(),
    }
}

fn options() -> RealmOptions {
    RealmOptions {
        timeout: Duration::from_secs(30),
        timers: false,
        codecs: false,
        clock_ms: None,
        random_seed: None,
        heap_limit_mb: None,
    }
}

#[test]
fn the_graph_becomes_the_realm_it_was_captured_from() {
    let page = GraphPage {
        url: "https://example.test/buy".to_string(),
        frames: 1,
        ..GraphPage::default()
    };

    let mut graph = open(&tiny(), &page, Hooks::default(), options()).expect("the sandbox opened");

    assert_eq!(
        graph.read("navigator.userAgent").unwrap(),
        json!(USER_AGENT)
    );
    assert_eq!(
        graph.read("navigator.hardwareConcurrency").unwrap(),
        json!(10)
    );
    assert_eq!(
        graph.read("navigator.vendor").unwrap(),
        json!("Google Inc.")
    );
    assert_eq!(graph.read("globalThis.innerWidth").unwrap(), json!(1512));
    assert_eq!(graph.read("globalThis.closed").unwrap(), json!(false));
}

#[test]
fn nothing_the_capture_left_out_is_invented() {
    let mut graph =
        open(&tiny(), &GraphPage::default(), Hooks::default(), options()).expect("sandbox");

    assert_eq!(graph.read("typeof document").unwrap(), json!("undefined"));
    assert_eq!(
        graph.read("typeof HTMLCanvasElement").unwrap(),
        json!("undefined")
    );
}

#[test]
fn an_accessor_reports_itself_as_native_code() {
    let mut graph =
        open(&tiny(), &GraphPage::default(), Hooks::default(), options()).expect("sandbox");

    let source = graph
        .read(
            "Function.prototype.toString.call(Object.getOwnPropertyDescriptor(Navigator.prototype, 'vendor').get)",
        )
        .unwrap();

    assert_eq!(source, json!("function get vendor() { [native code] }"));
}

#[test]
fn entropy_and_digests_come_from_the_host() {
    let mut graph =
        open(&tiny(), &GraphPage::default(), Hooks::default(), options()).expect("sandbox");

    let first = graph.read("crypto.randomUUID()").unwrap();
    let second = graph.read("crypto.randomUUID()").unwrap();

    assert_ne!(first, second);
    assert_eq!(first.as_str().map(str::len), Some(36));

    let bytes = graph
        .read("Array.from(crypto.getRandomValues(new Uint8Array(16))).length")
        .unwrap();

    assert_eq!(bytes, json!(16));
}

#[test]
fn a_script_runs_under_its_own_url() {
    let mut graph =
        open(&tiny(), &GraphPage::default(), Hooks::default(), options()).expect("sandbox");

    graph
        .eval(
            "globalThis.__where = new Error('x').stack;",
            "https://example.test/agent.js",
        )
        .expect("ran");

    let stack = graph.read("globalThis.__where").unwrap();
    let text = stack.as_str().unwrap_or_default();

    assert!(
        text.contains("https://example.test/agent.js"),
        "the stack does not name the script: {text}"
    );
    assert!(
        !text.contains("wre:"),
        "the stack names the harness: {text}"
    );
}
