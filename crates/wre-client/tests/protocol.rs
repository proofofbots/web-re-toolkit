use std::io::Cursor;

use serde_json::{Value, json};

use wre_client::client::prepare_params;
use wre_client::diag::{DiagConfig, DiagMode, Recorder, scrub};
use wre_client::error::{ClientError, ErrorKind};
use wre_client::proto::{Envelope, Frame, read_frame, write_frame};
use wre_client::shape::{Shape, apply_defaults, field, validate};
use wre_client::spec::{ClientDescriptor, OpSpec, PROTOCOL_VERSION};

fn descriptor() -> ClientDescriptor {
    ClientDescriptor::new("demo", "0.1.0")
        .config(Shape::object("Config", [field("key", Shape::Str).with_default(json!("k"))]))
        .op(OpSpec::new(
            "solve",
            Shape::object(
                "Facts",
                [
                    field("url", Shape::Str),
                    field("mode", Shape::enumeration("Mode", &["fast", "slow"]))
                        .with_default(json!("fast")),
                    field("tags", Shape::optional(Shape::list(Shape::Str))),
                ],
            ),
            Shape::object("Solved", [field("body", Shape::Bytes)]),
        ))
        .seal()
        .expect("descriptor seals")
}

#[test]
fn frames_round_trip_with_a_binary_part() {
    let envelope = Envelope::request(7, "solve", json!({ "url": "https://acme.example/" }));
    let frame = Frame::from_envelope(&envelope).unwrap().with_bin(vec![1, 2, 3, 4]);

    let mut buffer = Vec::new();
    write_frame(&mut buffer, &frame).unwrap();

    let mut cursor = Cursor::new(buffer);
    let read = read_frame(&mut cursor).unwrap().expect("one frame");

    assert_eq!(read.bin, vec![1, 2, 3, 4]);
    assert_eq!(read.envelope().unwrap(), envelope);
    assert!(read_frame(&mut cursor).unwrap().is_none());
}

#[test]
fn frames_survive_a_reader_that_returns_one_byte_at_a_time() {
    struct Trickle(Vec<u8>, usize);

    impl std::io::Read for Trickle {
        fn read(&mut self, out: &mut [u8]) -> std::io::Result<usize> {
            if self.1 >= self.0.len() || out.is_empty() {
                return Ok(0);
            }
            out[0] = self.0[self.1];
            self.1 += 1;
            Ok(1)
        }
    }

    let frame = Frame::from_envelope(&Envelope::event(3, "progress", json!({ "done": 1 })))
        .unwrap()
        .with_bin(vec![9; 32]);

    let mut buffer = Vec::new();
    write_frame(&mut buffer, &frame).unwrap();

    let mut reader = Trickle(buffer, 0);
    let read = read_frame(&mut reader).unwrap().expect("one frame");

    assert_eq!(read.bin.len(), 32);
}

#[test]
fn the_wire_keeps_its_field_names() {
    let envelope = Envelope::Req {
        v: PROTOCOL_VERSION,
        id: 2,
        op: "solve".to_string(),
        session: Some("s1".to_string()),
        params: json!({}),
        deadline_ms: Some(1500),
    };

    let encoded: Value = serde_json::from_slice(&serde_json::to_vec(&envelope).unwrap()).unwrap();

    assert_eq!(encoded["t"], "req");
    assert_eq!(encoded["v"], PROTOCOL_VERSION);
    assert_eq!(encoded["deadline_ms"], 1500);
    assert_eq!(encoded["session"], "s1");

    let failed = Envelope::failed(2, ClientError::timeout("too slow"), 12);
    let encoded: Value = serde_json::from_slice(&serde_json::to_vec(&failed).unwrap()).unwrap();

    assert_eq!(encoded["t"], "res");
    assert_eq!(encoded["ok"], false);
    assert_eq!(encoded["error"]["kind"], "timeout");
    assert_eq!(encoded["error"]["retryable"], true);
    assert_eq!(encoded["took_ms"], 12);
}

#[test]
fn validation_reports_every_problem_at_once() {
    let descriptor = descriptor();
    let shape = &descriptor.find("solve").unwrap().params;

    let problems = validate(shape, &json!({ "mode": "sideways", "extra": 1 }), &descriptor.types);

    assert!(problems.iter().any(|item| item.contains("url is required")));
    assert!(problems.iter().any(|item| item.contains("not one of Mode")));
    assert!(problems.iter().any(|item| item.contains("unknown field extra")));
}

#[test]
fn declared_defaults_are_filled_in_before_validation() {
    let descriptor = descriptor();
    let prepared =
        prepare_params(&descriptor, "solve", json!({ "url": "https://acme.example/" })).unwrap();

    assert_eq!(prepared["mode"], "fast");

    let mut value = json!({});
    apply_defaults(&descriptor.config, &mut value, &descriptor.types);
    assert_eq!(value["key"], "k");
}

#[test]
fn an_unknown_op_is_unsupported_and_names_the_ones_that_exist() {
    let descriptor = descriptor();
    let error = prepare_params(&descriptor, "nope", json!({})).unwrap_err();

    assert_eq!(error.kind, ErrorKind::Unsupported);
    assert!(error.message.contains("solve"));
}

#[test]
fn bytes_fields_only_accept_base64() {
    let shape = Shape::object("Body", [field("blob", Shape::Bytes)]);
    let types = Default::default();

    assert!(validate(&shape, &json!({ "blob": "aGk=" }), &types).is_empty());
    assert!(!validate(&shape, &json!({ "blob": "not base64 !!" }), &types).is_empty());
}

#[test]
fn the_schema_hash_moves_with_the_surface_and_not_with_the_version() {
    let mut bundle = wre_client::spec::BundleDescriptor {
        protocol: PROTOCOL_VERSION,
        bundle: "default".to_string(),
        toolkit_version: "0.1.0".to_string(),
        binary_version: "0.1.0".to_string(),
        clients: vec![descriptor()],
    };

    let before = bundle.schema_hash();

    bundle.binary_version = "9.9.9".to_string();
    assert_eq!(before, bundle.schema_hash());

    bundle.clients[0] = bundle.clients[0]
        .clone()
        .op(OpSpec::new("extra", Shape::object("ExtraInput", []), Shape::Unit))
        .seal()
        .unwrap();

    assert_ne!(before, bundle.schema_hash());
}

#[test]
fn a_descriptor_with_two_shapes_under_one_name_does_not_seal() {
    let clash = ClientDescriptor::new("clash", "0.1.0")
        .op(OpSpec::new(
            "a",
            Shape::object("Same", [field("one", Shape::Str)]),
            Shape::Unit,
        ))
        .op(OpSpec::new(
            "b",
            Shape::object("Same", [field("two", Shape::Int)]),
            Shape::Unit,
        ))
        .seal();

    assert!(clash.is_err());
}

#[test]
fn diagnostics_redact_secrets_and_truncate_long_values() {
    let scrubbed = scrub(
        &json!({
            "proxy": "http://user:pass@host:1080",
            "nested": { "authorization": "Bearer abc", "keep": "visible" },
            "long": "x".repeat(200),
        }),
        64,
    );

    assert!(scrubbed["proxy"].as_str().unwrap().starts_with("redacted:"));
    assert!(scrubbed["nested"]["authorization"].as_str().unwrap().starts_with("redacted:"));
    assert_eq!(scrubbed["nested"]["keep"], "visible");
    assert!(scrubbed["long"].as_str().unwrap().contains("truncated 200 bytes"));
}

#[test]
fn the_recorder_keeps_the_newest_events_and_counts_the_rest() {
    let recorder = Recorder::new(
        DiagConfig { mode: DiagMode::Always, max_events: 3, ..DiagConfig::default() },
        "demo",
        "s1",
    );

    for index in 0..10 {
        recorder.record("info", "step", &format!("step {index}"), json!({ "index": index }));
    }

    let report = recorder.report("test", None, Value::Null);

    assert_eq!(report.events.len(), 3);
    assert_eq!(report.dropped_events, 7);
    assert_eq!(report.events.last().unwrap().message, "step 9");
}

#[test]
fn a_report_is_written_only_when_the_mode_asks_for_it() {
    let off = Recorder::new(
        DiagConfig { mode: DiagMode::Off, ..DiagConfig::default() },
        "demo",
        "s1",
    );
    assert!(!off.should_write(true));

    let on_error = Recorder::new(
        DiagConfig { mode: DiagMode::OnError, ..DiagConfig::default() },
        "demo",
        "s1",
    );
    assert!(on_error.should_write(true));
    assert!(!on_error.should_write(false));

    let always = Recorder::new(
        DiagConfig { mode: DiagMode::Always, ..DiagConfig::default() },
        "demo",
        "s1",
    );
    assert!(always.should_write(false));
}

#[test]
fn writing_a_report_prunes_the_oldest_files() {
    let dir = std::env::temp_dir().join(format!("wre-diag-test-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);

    let recorder = Recorder::new(
        DiagConfig { mode: DiagMode::Always, keep_files: 2, ..DiagConfig::default() },
        "demo",
        "s1",
    );

    for index in 0..4 {
        let report = recorder.report(&format!("run {index}"), None, Value::Null);
        recorder.write(&report, &dir).unwrap();
        std::thread::sleep(std::time::Duration::from_millis(1100));
    }

    let count = std::fs::read_dir(&dir).unwrap().count();
    assert_eq!(count, 2);

    let _ = std::fs::remove_dir_all(&dir);
}

fn alpha() -> ClientDescriptor {
    ClientDescriptor::new("alpha", "1.0.0")
        .op(OpSpec::new("ping", Shape::object("PingInput", []), Shape::Str))
}

fn beta() -> ClientDescriptor {
    ClientDescriptor::new("beta", "1.0.0")
        .op(OpSpec::new("pong", Shape::object("PongInput", []), Shape::Str))
}

struct Stub;

impl wre_client::client::Client for Stub {
    fn call(&mut self, _op: &str, _params: Value, _call: &wre_client::context::Call) -> wre_client::error::ClientResult<Value> {
        Ok(json!("ok"))
    }
}

fn stub(_ctx: wre_client::context::Ctx, _config: Value) -> wre_client::error::ClientResult<Box<dyn wre_client::client::Client>> {
    Ok(Box::new(Stub))
}

#[test]
fn one_registry_can_hold_several_targets_but_not_the_same_one_twice() {
    use wre_client::client::{Registration, Registry};

    let mut registry = Registry::new();

    registry
        .register(Registration { id: "alpha", describe: alpha, build: stub })
        .expect("alpha registers");
    registry
        .register(Registration { id: "beta", describe: beta, build: stub })
        .expect("beta registers");

    assert_eq!(registry.ids(), vec!["alpha".to_string(), "beta".to_string()]);
    assert!(registry.descriptor("alpha").is_some());

    let again = registry.register(Registration { id: "alpha", describe: alpha, build: stub });
    assert!(again.is_err());

    let mismatch = registry.register(Registration { id: "gamma", describe: alpha, build: stub });
    assert!(mismatch.is_err());
}

#[test]
fn building_a_target_that_is_not_in_the_bundle_says_what_is() {
    use wre_client::client::{Registration, Registry};
    use wre_client::context::{Ctx, Services};

    let mut registry = Registry::new();
    registry
        .register(Registration { id: "alpha", describe: alpha, build: stub })
        .unwrap();

    let services = Services::detached().unwrap();
    let ctx = Ctx::new("beta", "s1", services);

    let error = match registry.build("beta", ctx, json!({})) {
        Ok(_) => panic!("beta is not in this registry"),
        Err(error) => error,
    };

    assert_eq!(error.kind, ErrorKind::Unsupported);
    assert!(error.message.contains("alpha"));
}
