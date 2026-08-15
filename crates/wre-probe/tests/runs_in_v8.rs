use wre_live::realm::{Realm, RealmOptions};
use wre_probe::{ProbeDump, SurfaceSpec, fingerprint_surface};

const FAKE_DOM: &str = r#"
globalThis.window = globalThis;

function Navigator() {}
Navigator.prototype = {};
Object.defineProperty(Navigator.prototype, "userAgent", {
  get: function () { return "Mozilla/5.0 pretend"; },
  configurable: true,
  enumerable: true
});
Object.defineProperty(Navigator.prototype, "hardwareConcurrency", {
  value: 8,
  configurable: true,
  writable: true,
  enumerable: true
});
globalThis.Navigator = Navigator;
globalThis.navigator = Object.create(Navigator.prototype);

function HTMLCanvasElement() {}
HTMLCanvasElement.prototype = {
  toDataURL: function (kind) { return "data:" + (kind || "image/png") + ";base64,AAAA"; }
};
globalThis.HTMLCanvasElement = HTMLCanvasElement;

function EventTarget() {}
EventTarget.prototype = { addEventListener: function () {} };
globalThis.EventTarget = EventTarget;

globalThis.fetch = function (url, init) {
  return Promise.resolve({ ok: true, url: url, init: init });
};
"#;

fn realm_with_dom() -> Realm {
    let mut realm = Realm::new(RealmOptions::default()).expect("realm");
    realm.eval_unit(FAKE_DOM, "fake-dom").expect("fake dom");
    realm
}

#[test]
fn the_generated_script_parses_and_installs() {
    let mut realm = realm_with_dom();

    let spec = SurfaceSpec::default()
        .property("Navigator.prototype", "userAgent")
        .property("Navigator.prototype", "hardwareConcurrency")
        .method("HTMLCanvasElement.prototype", "toDataURL");

    realm.eval_unit(&spec.build().unwrap(), "probe").expect("probe installed");

    let installed = realm.eval_json("__WRE.installed").unwrap();
    assert_eq!(installed["properties"], 2);
    assert_eq!(installed["methods"], 1);
    assert_eq!(installed["failed"], 0);
}

#[test]
fn records_property_reads_and_method_calls() {
    let mut realm = realm_with_dom();

    let spec = SurfaceSpec::default()
        .property("Navigator.prototype", "userAgent")
        .method("HTMLCanvasElement.prototype", "toDataURL");

    realm.eval_unit(&spec.build().unwrap(), "probe").unwrap();

    let value = realm
        .eval_json(
            r#"(function () {
                var seen = navigator.userAgent + navigator.userAgent;
                var canvas = Object.create(HTMLCanvasElement.prototype);
                var url = canvas.toDataURL("image/webp");
                return { seen: seen.length, url: url };
            })()"#,
        )
        .unwrap();

    assert!(value["url"].as_str().unwrap().starts_with("data:image/webp"));

    let dump = ProbeDump::parse(&realm.eval_json(&spec.dump_expression()).unwrap()).unwrap();

    let read = dump
        .reads
        .iter()
        .find(|entry| entry.key == "Navigator.prototype.userAgent")
        .expect("userAgent read recorded");
    assert_eq!(read.count, 2);
    assert_eq!(read.samples[0], "Mozilla/5.0 pretend");

    let call = dump
        .calls
        .iter()
        .find(|entry| entry.key == "HTMLCanvasElement.prototype.toDataURL")
        .expect("toDataURL call recorded");
    assert_eq!(call.count, 1);
    assert_eq!(call.samples[0], "image/webp");
    assert!(call.results[0].starts_with("data:image/webp"));
}

#[test]
fn a_wrapped_function_still_reports_as_native() {
    let mut realm = realm_with_dom();

    let spec = SurfaceSpec {
        stealth: true,
        ..SurfaceSpec::default()
    }
    .method("HTMLCanvasElement.prototype", "toDataURL");

    realm.eval_unit(&spec.build().unwrap(), "probe").unwrap();

    let text = realm
        .eval_json("String(HTMLCanvasElement.prototype.toDataURL)")
        .unwrap();

    assert!(
        !text.as_str().unwrap().contains("bump("),
        "the wrapper leaked its body: {text}"
    );

    let name = realm.eval_json("HTMLCanvasElement.prototype.toDataURL.name").unwrap();
    assert_eq!(name, serde_json::json!("toDataURL"));
}

#[test]
fn records_network_calls() {
    let mut realm = realm_with_dom();

    let spec = SurfaceSpec { network: true, ..SurfaceSpec::default() };
    realm.eval_unit(&spec.build().unwrap(), "probe").unwrap();

    realm
        .eval_unit("fetch('https://target.test/collect', { method: 'POST', body: 'payload' });", "run")
        .unwrap();

    let dump = ProbeDump::parse(&realm.eval_json(&spec.dump_expression()).unwrap()).unwrap();
    let posts = dump.posts();

    assert_eq!(posts.len(), 1);
    assert_eq!(posts[0]["url"], "https://target.test/collect");
    assert_eq!(posts[0]["body"], "payload");
}

#[test]
fn missing_surfaces_are_reported_not_fatal() {
    let mut realm = realm_with_dom();

    let spec = SurfaceSpec::default()
        .property("NoSuchThing.prototype", "nope")
        .method("AlsoMissing.prototype", "gone");

    realm.eval_unit(&spec.build().unwrap(), "probe").unwrap();

    let dump = ProbeDump::parse(&realm.eval_json(&spec.dump_expression()).unwrap()).unwrap();
    assert_eq!(dump.notes.len(), 2);
    assert!(dump.notes.iter().any(|note| note.contains("NoSuchThing")));
}

#[test]
fn the_fingerprint_preset_installs_what_exists() {
    let mut realm = realm_with_dom();
    let spec = fingerprint_surface();

    realm
        .eval_unit(&spec.build().unwrap(), "probe")
        .expect("preset script runs");

    let installed = realm.eval_json("__WRE.installed").unwrap();
    assert!(installed["properties"].as_u64().unwrap() >= 2);
    assert!(installed["failed"].as_u64().unwrap() > 0);
}

#[test]
fn reset_clears_the_records() {
    let mut realm = realm_with_dom();
    let spec = SurfaceSpec::default().property("Navigator.prototype", "userAgent");

    realm.eval_unit(&spec.build().unwrap(), "probe").unwrap();
    realm.eval_unit("navigator.userAgent;", "run").unwrap();
    realm.eval_unit("__WRE.reset();", "reset").unwrap();

    let dump = ProbeDump::parse(&realm.eval_json(&spec.dump_expression()).unwrap()).unwrap();
    assert!(dump.reads.is_empty());
}
