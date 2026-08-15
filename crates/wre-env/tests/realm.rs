use serde_json::json;

use wre_env::{
    CaptureOptions, MaterializeOptions, Snapshot, capture_script, materialize, synthetic_snapshot,
};
use wre_live::realm::{Realm, RealmOptions};

#[test]
fn materialises_a_snapshot_into_a_realm() {
    let mut realm = Realm::new(RealmOptions::default()).unwrap();
    let snapshot = synthetic_snapshot();

    let report = materialize(&mut realm, &snapshot, &MaterializeOptions::default()).unwrap();

    assert_eq!(report.objects, 3);
    assert!(report.roots.contains(&"navigator".to_string()));
    assert!(report.missing.is_empty(), "{:?}", report.missing);

    assert_eq!(
        realm.eval_json("navigator.platform").unwrap(),
        json!("MacIntel")
    );
    assert_eq!(realm.eval_json("navigator.hardwareConcurrency").unwrap(), json!(8));
    assert_eq!(realm.eval_json("navigator.webdriver").unwrap(), json!(false));
    assert_eq!(realm.eval_json("screen.width").unwrap(), json!(1512));
}

#[test]
fn nested_references_materialise_lazily() {
    let mut realm = Realm::new(RealmOptions::default()).unwrap();
    materialize(&mut realm, &synthetic_snapshot(), &MaterializeOptions::default()).unwrap();

    assert_eq!(
        realm.eval_json("navigator.languages[0]").unwrap(),
        json!("en-US")
    );
    assert_eq!(realm.eval_json("navigator.languages.length").unwrap(), json!(2));
    assert_eq!(
        realm.eval_json("Array.isArray(navigator.languages)").unwrap(),
        json!(true)
    );
}

#[test]
fn window_self_and_top_point_at_the_realm() {
    let mut realm = Realm::new(RealmOptions::default()).unwrap();
    materialize(&mut realm, &synthetic_snapshot(), &MaterializeOptions::default()).unwrap();

    assert_eq!(realm.eval_json("window === globalThis").unwrap(), json!(true));
    assert_eq!(realm.eval_json("self === top").unwrap(), json!(true));
}

#[test]
fn a_captured_snapshot_can_be_replayed() {
    let mut source = Realm::new(RealmOptions::default()).unwrap();

    source
        .eval_unit(
            r#"
            globalThis.navigator = {
                userAgent: "pretend/1.0",
                platform: "TestPlatform",
                languages: ["fr-FR", "fr"],
                hardwareConcurrency: 12
            };
            globalThis.screen = { width: 800, height: 600, colorDepth: 30 };
            "#,
            "fake-browser",
        )
        .unwrap();

    let options = CaptureOptions {
        depth: 4,
        roots: vec!["navigator".to_string(), "screen".to_string()],
        ..CaptureOptions::default()
    };

    let raw = source.eval(&capture_script(&options).unwrap(), "capture").unwrap();
    let snapshot = Snapshot::parse(&raw).unwrap();

    assert_eq!(snapshot.read("navigator.platform"), Some(json!("TestPlatform")));
    assert!(!snapshot.objects.is_empty());

    let mut target = Realm::new(RealmOptions::default()).unwrap();
    materialize(&mut target, &snapshot, &MaterializeOptions::default()).unwrap();

    assert_eq!(target.eval_json("navigator.userAgent").unwrap(), json!("pretend/1.0"));
    assert_eq!(target.eval_json("navigator.languages[1]").unwrap(), json!("fr"));
    assert_eq!(target.eval_json("screen.colorDepth").unwrap(), json!(30));
    assert_eq!(target.eval_json("navigator.hardwareConcurrency").unwrap(), json!(12));
}

#[test]
fn captured_functions_become_callable_stubs() {
    let mut source = Realm::new(RealmOptions::default()).unwrap();

    source
        .eval_unit(
            "globalThis.navigator = { sendBeacon: function sendBeacon(url, data) { return true; } };",
            "fake-browser",
        )
        .unwrap();

    let options = CaptureOptions {
        roots: vec!["navigator".to_string()],
        ..CaptureOptions::default()
    };

    let raw = source.eval(&capture_script(&options).unwrap(), "capture").unwrap();
    let snapshot = Snapshot::parse(&raw).unwrap();
    assert!(snapshot.function_count() >= 1);

    let mut target = Realm::new(RealmOptions::default()).unwrap();
    materialize(&mut target, &snapshot, &MaterializeOptions::default()).unwrap();

    assert_eq!(
        target.eval_json("typeof navigator.sendBeacon").unwrap(),
        json!("function")
    );
    assert_eq!(
        target.eval_json("navigator.sendBeacon.name").unwrap(),
        json!("sendBeacon")
    );
    assert_eq!(
        target.eval_json("navigator.sendBeacon('u', 'd')").unwrap(),
        json!(null)
    );
}

#[test]
fn a_bridge_answers_calls_the_snapshot_cannot() {
    let mut realm = Realm::new(RealmOptions::default()).unwrap();

    realm
        .register_host(
            "__hostBridge",
            Box::new(|args| {
                let name = args
                    .first()
                    .and_then(|value| value.as_str())
                    .unwrap_or_default()
                    .to_string();
                Ok(json!(format!("bridged:{name}")))
            }),
        )
        .unwrap();

    let mut source = Realm::new(RealmOptions::default()).unwrap();
    source
        .eval_unit(
            "globalThis.navigator = { getBattery: function getBattery() { return null; } };",
            "fake-browser",
        )
        .unwrap();

    let options = CaptureOptions {
        roots: vec!["navigator".to_string()],
        ..CaptureOptions::default()
    };
    let raw = source.eval(&capture_script(&options).unwrap(), "capture").unwrap();
    let snapshot = Snapshot::parse(&raw).unwrap();

    materialize(
        &mut realm,
        &snapshot,
        &MaterializeOptions {
            record_calls: false,
            bridge: Some("__hostBridge".to_string()),
        },
    )
    .unwrap();

    assert_eq!(
        realm.eval_json("navigator.getBattery()").unwrap(),
        json!("bridged:getBattery")
    );
}

#[test]
fn a_target_script_reads_the_materialised_environment() {
    let mut realm = Realm::new(RealmOptions::default()).unwrap();
    materialize(&mut realm, &synthetic_snapshot(), &MaterializeOptions::default()).unwrap();

    let value = realm
        .eval(
            r#"
            (function collect() {
                return [
                    navigator.userAgent.indexOf("Chrome") > 0,
                    navigator.platform,
                    navigator.languages.join(","),
                    screen.width + "x" + screen.height,
                    navigator.webdriver
                ];
            })()
            "#,
            "target",
        )
        .unwrap();

    assert_eq!(
        value,
        json!([true, "MacIntel", "en-US,en", "1512x982", false])
    );
}
