use std::time::Duration;

use serde_json::json;

use wre_js::surface::SignatureRule;
use wre_live::mount::{MountPlan, SourcePatch, apply_patches, mount};
use wre_live::realm::{Realm, RealmOptions};

fn realm() -> Realm {
    Realm::new(RealmOptions {
        timeout: Duration::from_secs(5),
        clock_ms: Some(1_700_000_000_000.0),
        random_seed: Some(42),
        ..RealmOptions::default()
    })
    .expect("realm started")
}

#[test]
fn evaluates_and_returns_json() {
    let mut realm = realm();
    let value = realm.eval("({ a: 1, b: [2, 3], c: 'x' })", "test").unwrap();
    assert_eq!(value, json!({ "a": 1, "b": [2, 3], "c": "x" }));
}

#[test]
fn pins_the_clock() {
    let mut realm = realm();
    let first = realm.eval("Date.now()", "test").unwrap();
    let second = realm.eval("Date.now()", "test").unwrap();
    assert_eq!(first, json!(1_700_000_000_000i64));
    assert_eq!(first, second);
    assert_eq!(realm.eval("new Date().getTime()", "test").unwrap(), first);
}

#[test]
fn seeds_random_deterministically() {
    let mut first = realm();
    let mut second = realm();
    let a = first
        .eval("[Math.random(), Math.random()]", "test")
        .unwrap();
    let b = second
        .eval("[Math.random(), Math.random()]", "test")
        .unwrap();
    assert_eq!(a, b);
    assert_ne!(a[0], a[1]);
}

#[test]
fn captures_console_output() {
    let mut realm = realm();
    realm
        .eval_unit("console.log('hello', 42); console.warn('careful');", "test")
        .unwrap();

    let records = realm.records().unwrap();
    assert_eq!(records.console.len(), 2);
    assert_eq!(records.console[0].text, "hello 42");
    assert_eq!(records.console[1].level, "warn");

    let drained = realm.records().unwrap();
    assert!(drained.console.is_empty());
}

#[test]
fn reports_thrown_errors_with_context() {
    let mut realm = realm();
    let error = realm
        .eval_unit(
            "function boom() { throw new Error('kaboom'); } boom();",
            "test",
        )
        .unwrap_err();
    let text = error.to_string();
    assert!(text.contains("kaboom"), "{text}");
}

#[test]
fn stops_runaway_execution() {
    let mut realm = Realm::new(RealmOptions {
        timeout: Duration::from_millis(300),
        ..RealmOptions::default()
    })
    .unwrap();

    let error = realm.eval_unit("while (true) {}", "spin").unwrap_err();
    assert!(error.to_string().contains("budget"), "{error}");
}

#[test]
fn captures_and_calls_a_function_handle() {
    let mut realm = realm();
    realm
        .eval_unit(
            "globalThis.mix = function (a, b) { return a * 10 + b.length; };",
            "test",
        )
        .unwrap();

    let handle = realm.capture("mix", "globalThis.mix").unwrap();
    let value = realm.call(&handle, &[json!(4), json!("abc")]).unwrap();
    assert_eq!(value, json!(43));
}

#[test]
fn host_functions_cross_the_boundary() {
    let mut realm = realm();

    realm
        .register_host(
            "__hostSum",
            Box::new(|args| {
                let total: f64 = args.iter().filter_map(|value| value.as_f64()).sum();
                Ok(json!(total))
            }),
        )
        .unwrap();

    let value = realm.eval("__hostSum(1, 2, 3.5)", "test").unwrap();
    assert_eq!(value, json!(6.5));
}

#[test]
fn host_errors_surface_as_javascript_exceptions() {
    let mut realm = realm();

    realm
        .register_host(
            "__hostFail",
            Box::new(|_| Err(wre_core::error::Error::msg("no such device"))),
        )
        .unwrap();

    let value = realm
        .eval("(() => { try { __hostFail(); return 'no throw'; } catch (e) { return e.message; } })()", "test")
        .unwrap();

    assert!(
        value
            .as_str()
            .unwrap_or_default()
            .contains("no such device"),
        "{value}"
    );
}

#[test]
fn runs_queued_timers_on_demand() {
    let mut realm = realm();
    realm
        .eval_unit(
            "globalThis.hits = 0; setTimeout(function () { hits++; setTimeout(function () { hits += 10; }, 0); }, 5);",
            "test",
        )
        .unwrap();

    assert_eq!(realm.pending_timers().unwrap(), 1);
    let ran = realm.run_timers(4).unwrap();
    assert_eq!(ran, 2);
    assert_eq!(realm.eval("hits", "test").unwrap(), json!(11));
}

#[test]
fn records_property_access_through_a_watch() {
    let mut realm = realm();
    realm
        .eval_unit(
            "globalThis.navigator = { userAgent: 'ua', platform: 'MacIntel' };",
            "test",
        )
        .unwrap();

    assert!(realm.watch("globalThis", "navigator", "navigator").unwrap());
    realm
        .eval_unit(
            "var a = navigator.userAgent; var b = navigator.platform;",
            "test",
        )
        .unwrap();

    let records = realm.records().unwrap();
    let keys: Vec<&str> = records
        .access
        .iter()
        .map(|entry| entry.key.as_str())
        .collect();
    assert!(keys.contains(&"userAgent"), "{keys:?}");
    assert!(keys.contains(&"platform"), "{keys:?}");
}

#[test]
fn records_traced_calls() {
    let mut realm = realm();
    realm
        .eval_unit(
            "globalThis.holder = { encode: function (text) { return text.toUpperCase(); } };",
            "test",
        )
        .unwrap();

    assert!(
        realm
            .trace("globalThis.holder", "encode", "encode")
            .unwrap()
    );
    let value = realm.eval("holder.encode('ab')", "test").unwrap();
    assert_eq!(value, json!("AB"));

    let records = realm.records().unwrap();
    assert_eq!(records.calls.len(), 1);
    assert_eq!(records.calls[0].args, vec!["ab".to_string()]);
    assert_eq!(records.calls[0].result.as_deref(), Some("AB"));
}

#[test]
fn base64_helpers_are_present() {
    let mut realm = realm();
    assert_eq!(realm.eval("btoa('hi')", "test").unwrap(), json!("aGk="));
    assert_eq!(realm.eval("atob('aGk=')", "test").unwrap(), json!("hi"));
}

#[test]
fn applies_source_patches() {
    let source = "var mode = 'live'; function go() { return mode; }";
    let (patched, applied) =
        apply_patches(source, &[SourcePatch::literal("'live'", "'offline'")]).unwrap();

    assert_eq!(applied, 1);
    assert!(patched.contains("'offline'"));
}

#[test]
fn a_required_patch_that_misses_is_an_error() {
    let error = apply_patches("var a = 1;", &[SourcePatch::literal("nope", "x")]).unwrap_err();
    assert!(error.to_string().contains("did not match"), "{error}");
}

#[test]
fn an_optional_patch_that_misses_is_fine() {
    let (out, applied) = apply_patches(
        "var a = 1;",
        &[SourcePatch::literal("nope", "x").optional()],
    )
    .unwrap();
    assert_eq!(applied, 0);
    assert_eq!(out, "var a = 1;");
}

#[test]
fn mounts_a_target_and_borrows_its_primitives() {
    let target = r#"
        function rotate(text, shift) {
            var out = "";
            for (var i = 0; i < text.length; i++) {
                out += String.fromCharCode(text.charCodeAt(i) ^ shift);
            }
            return out;
        }

        function checksum(bytes) {
            var hash = 0x811c9dc5;
            for (var i = 0; i < bytes.length; i++) {
                hash ^= bytes.charCodeAt(i);
                hash = Math.imul(hash, 0x01000193) >>> 0;
            }
            return hash >>> 0;
        }

        var secret = rotate("hello", 3);
    "#;

    let plan = MountPlan {
        signatures: vec![
            SignatureRule {
                role: "rotate".to_string(),
                pattern: r"String\.fromCharCode".to_string(),
                params: Some(2),
            },
            SignatureRule {
                role: "checksum".to_string(),
                pattern: r"2166136261|0x811c9dc5".to_string(),
                params: Some(1),
            },
        ],
        ..MountPlan::default()
    };

    let mut mounted = mount(target, &plan, RealmOptions::default()).expect("mounted");

    assert_eq!(
        mounted.roles(),
        vec!["checksum".to_string(), "rotate".to_string()]
    );

    let rotated = mounted.call("rotate", &[json!("hello"), json!(3)]).unwrap();
    let back = mounted
        .call("rotate", &[rotated.clone(), json!(3)])
        .unwrap();
    assert_eq!(back, json!("hello"));

    let digest = mounted.call("checksum", &[json!("hello")]).unwrap();
    assert_eq!(digest, json!(0x4f9f2cabu32));
}

#[test]
fn mount_can_export_by_expression() {
    let target = "var internals = { pack: function (a) { return [a, a]; } };";
    let plan = MountPlan::default().with_export("pack", "internals.pack");

    let mut mounted = mount(target, &plan, RealmOptions::default()).unwrap();
    let value = mounted.call("pack", &[json!(7)]).unwrap();
    assert_eq!(value, json!([7, 7]));
    assert_eq!(mounted.report.roles.get("pack"), Some(&true));
}

#[test]
fn mount_reports_a_target_that_throws_when_tolerated() {
    let target = "function keep(a) { return a + 1; } missingGlobal.boom();";

    let plan = MountPlan {
        tolerate_throw: true,
        signatures: vec![SignatureRule {
            role: "keep".to_string(),
            pattern: r"a \+ 1".to_string(),
            params: Some(1),
        }],
        ..MountPlan::default()
    };

    let mut mounted = mount(target, &plan, RealmOptions::default()).unwrap();
    assert_eq!(mounted.call("keep", &[json!(1)]).unwrap(), json!(2));
}

#[test]
fn lists_global_names() {
    let mut realm = realm();
    let names = realm.global_names().unwrap();
    assert!(names.iter().any(|name| name == "JSON"));
    assert!(names.iter().any(|name| name == "console"));
}

#[test]
fn a_frame_is_a_realm_of_its_own() {
    let mut realm = realm();
    let frame = realm.open_frame().unwrap();

    realm
        .eval_unit("globalThis.here = 'document';", "")
        .unwrap();
    realm
        .eval_unit_in(frame, "globalThis.here = 'frame';", "")
        .unwrap();

    assert_eq!(
        realm.eval_json("globalThis.here").unwrap(),
        json!("document")
    );
    assert_eq!(
        realm.eval_json_in(frame, "globalThis.here").unwrap(),
        json!("frame")
    );
    assert_eq!(realm.frame_count(), 1);

    let separate = realm
        .eval_json_in(frame, "globalThis.Object === undefined")
        .unwrap();
    assert_eq!(
        separate,
        json!(false),
        "a frame has its own builtins, not none"
    );
}

#[test]
fn a_frames_global_and_its_values_cross_into_the_document() {
    let mut realm = realm();
    let frame = realm.open_frame().unwrap();

    realm
        .eval_unit_in(frame, "globalThis.mark = { from: 'frame' };", "")
        .unwrap();

    realm.share_global(frame, None, "view").unwrap();
    realm
        .share_value(frame, "globalThis.mark", "carried")
        .unwrap();

    assert_eq!(realm.eval_json("view.mark.from").unwrap(), json!("frame"));
    assert_eq!(realm.eval_json("carried.from").unwrap(), json!("frame"));
    assert_eq!(
        realm.eval_json("view === globalThis").unwrap(),
        json!(false)
    );
    assert_eq!(
        realm.eval_json("view.Object === Object").unwrap(),
        json!(false),
        "the frame's builtins are not the document's"
    );
}

#[test]
fn one_table_can_be_shared_into_every_frame() {
    let mut realm = realm();
    let first = realm.open_frame().unwrap();
    let second = realm.open_frame().unwrap();

    realm
        .eval_unit("globalThis.shared = new WeakMap();", "")
        .unwrap();
    realm
        .share_into(first, "globalThis.shared", "shared")
        .unwrap();
    realm
        .share_into(second, "globalThis.shared", "shared")
        .unwrap();

    realm
        .eval_unit(
            "globalThis.key = {}; shared.set(globalThis.key, 'seen');",
            "",
        )
        .unwrap();
    realm
        .share_value(first, "globalThis.shared", "back")
        .unwrap();

    assert_eq!(
        realm.eval_json("back.get(globalThis.key)").unwrap(),
        json!("seen")
    );

    realm.share_into(second, "globalThis.key", "key").unwrap();
    assert_eq!(
        realm
            .eval_json_in(second, "shared.get(globalThis.key)")
            .unwrap(),
        json!("seen")
    );
}

#[test]
fn a_javascript_method_can_be_rebuilt_as_a_native_one() {
    let mut realm = realm();

    realm
        .eval_unit(
            "globalThis.Shell = function () {};
             Shell.prototype.make = function make(kind) { return 'made ' + kind; };",
            "",
        )
        .unwrap();

    realm
        .make_native("Shell.prototype", "make", Some("createElement"))
        .unwrap();

    assert_eq!(
        realm.eval_json("new Shell().make('div')").unwrap(),
        json!("made div"),
        "the replacement still forwards"
    );

    assert_eq!(
        realm
            .eval_json("Function.prototype.toString.call(Shell.prototype.make)")
            .unwrap(),
        json!("function createElement() { [native code] }")
    );

    assert_eq!(
        realm.eval_json("Shell.prototype.make.name").unwrap(),
        json!("createElement")
    );
    assert_eq!(
        realm.eval_json("Shell.prototype.make.length").unwrap(),
        json!(1)
    );

    let message = realm
        .eval_json(
            "(function () { try { class X extends Shell.prototype.make {} } catch (error) { return error.message; } return 'no throw'; })()",
        )
        .unwrap();

    assert_eq!(
        message,
        json!(
            "Class extends value function createElement() { [native code] } is not a constructor or null"
        )
    );
}

#[test]
fn a_rebuilt_method_still_throws_what_it_threw() {
    let mut realm = realm();

    realm
        .eval_unit(
            "globalThis.Shell = { fail: function () { throw new TypeError('Illegal invocation'); } };",
            "",
        )
        .unwrap();

    realm.make_native("Shell", "fail", None).unwrap();

    let message = realm
        .eval_json("(function () { try { Shell.fail(); } catch (error) { return error.message; } return 'no throw'; })()")
        .unwrap();

    assert_eq!(message, json!("Illegal invocation"));
}
