use serde_json::{Value, json};

use wre_live::realm::{Realm, RealmOptions};
use wre_sandbox::{Profile, Sandbox, install};

fn mounted() -> (Realm, Sandbox) {
    let mut realm = Realm::new(RealmOptions::default()).expect("realm");
    let sandbox = install(&mut realm, &Profile::desktop_chrome()).expect("install");
    (realm, sandbox)
}

fn ask(realm: &mut Realm, expression: &str) -> Value {
    realm.eval_json(expression).expect(expression)
}

#[test]
fn the_profile_values_are_what_the_page_reads() {
    let (mut realm, _) = mounted();

    assert_eq!(
        ask(&mut realm, "navigator.platform"),
        json!("MacIntel")
    );
    let profile = Profile::desktop_chrome();
    let reads = |brand: &str, name: &str| profile.property(brand, name).cloned().unwrap();

    assert_eq!(
        ask(&mut realm, "navigator.hardwareConcurrency"),
        reads("Navigator", "hardwareConcurrency")
    );
    assert_eq!(ask(&mut realm, "navigator.webdriver"), json!(false));
    assert_eq!(ask(&mut realm, "navigator.languages"), reads("Navigator", "languages"));
    assert_eq!(ask(&mut realm, "screen.width"), reads("Screen", "width"));
    assert_eq!(ask(&mut realm, "innerWidth"), reads("Window", "innerWidth"));
    assert_eq!(
        ask(&mut realm, "devicePixelRatio").as_f64(),
        Some(2.0)
    );
}

#[test]
fn every_accessor_is_a_real_native_function() {
    let (mut realm, _) = mounted();

    for (holder, property) in [
        ("Navigator", "userAgent"),
        ("Navigator", "webdriver"),
        ("Screen", "colorDepth"),
        ("Window", "innerWidth"),
    ] {
        let text = ask(
            &mut realm,
            &format!(
                "Object.getOwnPropertyDescriptor({holder}.prototype,'{property}').get.toString()"
            ),
        );

        assert!(
            text.as_str().unwrap().contains("[native code]"),
            "{holder}.{property} getter reads as {text}"
        );
    }
}

#[test]
fn function_prototype_tostring_is_never_patched() {
    let (mut realm, _) = mounted();

    let text = ask(&mut realm, "Function.prototype.toString.toString()");
    assert!(text.as_str().unwrap().contains("[native code]"), "{text}");

    assert_eq!(
        ask(
            &mut realm,
            "Object.getOwnPropertyDescriptor(Function.prototype,'toString').writable"
        ),
        json!(true)
    );

    assert_eq!(
        ask(&mut realm, "Function.prototype.toString.name"),
        json!("toString")
    );
}

#[test]
fn calling_an_accessor_on_the_wrong_receiver_throws_illegal_invocation() {
    let (mut realm, _) = mounted();

    let outcome = ask(
        &mut realm,
        "(function () { \
           try { \
             Object.getOwnPropertyDescriptor(Navigator.prototype,'userAgent').get.call({}); \
             return 'no throw'; \
           } catch (error) { \
             return error.constructor.name + ': ' + error.message; \
           } \
         })()",
    );

    assert_eq!(outcome, json!("TypeError: Illegal invocation"));
}

#[test]
fn the_same_accessor_still_works_on_a_real_receiver() {
    let (mut realm, _) = mounted();

    let outcome = ask(
        &mut realm,
        "Object.getOwnPropertyDescriptor(Navigator.prototype,'userAgent').get.call(navigator)",
    );

    assert!(outcome.as_str().unwrap().contains("Chrome/151"));
}

#[test]
fn the_prototype_chain_has_the_shape_a_real_browser_has() {
    let (mut realm, _) = mounted();

    assert_eq!(ask(&mut realm, "navigator instanceof Navigator"), json!(true));
    assert_eq!(
        ask(&mut realm, "Object.getPrototypeOf(navigator) === Navigator.prototype"),
        json!(true)
    );
    assert_eq!(
        ask(&mut realm, "Object.prototype.toString.call(navigator)"),
        json!("[object Navigator]")
    );
    assert_eq!(ask(&mut realm, "Navigator.prototype.constructor.name"), json!("Navigator"));
    assert_eq!(ask(&mut realm, "globalThis instanceof Window"), json!(true));
    assert_eq!(
        ask(&mut realm, "Object.getPrototypeOf(Window.prototype) === EventTarget.prototype"),
        json!(true)
    );
}

#[test]
fn the_properties_live_on_the_prototype_not_the_instance() {
    let (mut realm, _) = mounted();

    assert_eq!(
        ask(&mut realm, "Object.getOwnPropertyDescriptor(navigator,'userAgent') === undefined"),
        json!(true)
    );

    let descriptor = ask(
        &mut realm,
        "(function () { \
           var d = Object.getOwnPropertyDescriptor(Navigator.prototype,'userAgent'); \
           return { \
             hasGet: typeof d.get === 'function', \
             hasSet: d.set === undefined, \
             enumerable: d.enumerable, \
             configurable: d.configurable, \
             hasValue: 'value' in d \
           }; \
         })()",
    );

    assert_eq!(
        descriptor,
        json!({
            "hasGet": true,
            "hasSet": true,
            "enumerable": true,
            "configurable": true,
            "hasValue": false
        })
    );
}

#[test]
fn the_interface_constructors_refuse_to_be_called() {
    let (mut realm, _) = mounted();

    for name in ["Navigator", "Screen", "Plugin", "PluginArray", "MediaQueryList"] {
        let outcome = ask(
            &mut realm,
            &format!(
                "(function () {{ try {{ new {name}(); return 'no throw'; }} \
                  catch (error) {{ return error.message; }} }})()"
            ),
        );
        assert_eq!(outcome, json!("Illegal constructor"), "{name} was constructible");
    }
}

#[test]
fn webgl_answers_from_the_profile_and_records_what_it_cannot() {
    let (mut realm, sandbox) = mounted();

    let renderer = ask(
        &mut realm,
        "WebGLRenderingContext.prototype.getParameter.call(null, 37446)",
    );
    let profile = Profile::desktop_chrome();

    assert_eq!(Some(&renderer), profile.webgl_parameters.get("37446"));

    assert_eq!(
        ask(&mut realm, "WebGLRenderingContext.prototype.getSupportedExtensions().length"),
        json!(profile.webgl_extensions.len())
    );

    assert_eq!(
        ask(&mut realm, "WebGLRenderingContext.prototype.getParameter.call(null, 9999)"),
        Value::Null
    );

    assert!(
        sandbox
            .misses()
            .iter()
            .any(|entry| entry == "WebGLRenderingContext getParameter(9999)"),
        "{:?}",
        sandbox.misses()
    );
}

#[test]
fn the_debug_renderer_extension_is_present_and_reports_its_constants() {
    let (mut realm, _) = mounted();

    let extension = ask(
        &mut realm,
        "WebGLRenderingContext.prototype.getExtension.call(\
           WebGLRenderingContext.prototype, 'WEBGL_debug_renderer_info')",
    );

    assert_eq!(
        extension,
        json!({ "UNMASKED_VENDOR_WEBGL": 37445, "UNMASKED_RENDERER_WEBGL": 37446 })
    );

    assert_eq!(
        ask(
            &mut realm,
            "WebGLRenderingContext.prototype.getExtension.call(\
               WebGLRenderingContext.prototype, 'NOT_A_REAL_EXTENSION')"
        ),
        Value::Null
    );
}

#[test]
fn media_support_answers_from_the_profile() {
    let (mut realm, sandbox) = mounted();

    assert_eq!(
        ask(
            &mut realm,
            "HTMLMediaElement.prototype.canPlayType.call(null, 'audio/mpeg')"
        ),
        json!("probably")
    );

    assert_eq!(
        ask(
            &mut realm,
            "HTMLMediaElement.prototype.canPlayType.call(null, 'video/ogg; codecs=\"theora\"')"
        ),
        json!("")
    );

    assert_eq!(
        ask(
            &mut realm,
            "HTMLMediaElement.prototype.canPlayType.call(null, 'video/nonsense')"
        ),
        json!("")
    );

    assert!(sandbox.misses().iter().any(|entry| entry.contains("video/nonsense")));
}

#[test]
fn match_media_returns_a_media_query_list() {
    let (mut realm, _) = mounted();

    assert_eq!(ask(&mut realm, "matchMedia('(hover: hover)').matches"), json!(true));
    assert_eq!(
        ask(&mut realm, "matchMedia('(prefers-reduced-motion: reduce)').matches"),
        json!(false)
    );
    assert_eq!(
        ask(&mut realm, "matchMedia('(hover: hover)') instanceof MediaQueryList"),
        json!(true)
    );
    assert_eq!(ask(&mut realm, "matchMedia('(hover: hover)').media"), json!("(hover: hover)"));
}

#[test]
fn match_media_and_its_result_are_built_natively() {
    let (mut realm, _) = mounted();

    assert_eq!(
        ask(&mut realm, "matchMedia.toString().indexOf('[native code]') >= 0"),
        json!(true)
    );
    assert_eq!(ask(&mut realm, "matchMedia.name"), json!("matchMedia"));
    assert_eq!(ask(&mut realm, "matchMedia.length"), json!(1));
    assert_eq!(
        ask(&mut realm, "matchMedia.toString().indexOf('matchMedia') >= 0"),
        json!(true)
    );

    assert_eq!(
        ask(
            &mut realm,
            "Object.getOwnPropertyNames(matchMedia('(hover: hover)')).length"
        ),
        json!(0),
        "the values belong on the prototype, not on the instance"
    );

    assert_eq!(
        ask(
            &mut realm,
            "Object.getOwnPropertyDescriptor(MediaQueryList.prototype, 'media')\
             .get.toString().indexOf('[native code]') >= 0"
        ),
        json!(true)
    );

    assert_eq!(
        ask(
            &mut realm,
            "Object.prototype.toString.call(matchMedia('(hover: hover)'))"
        ),
        json!("[object MediaQueryList]")
    );

    assert_eq!(
        ask(&mut realm, "MediaQueryList.prototype instanceof EventTarget"),
        json!(true)
    );

    assert_eq!(
        ask(
            &mut realm,
            "(function () { try { return MediaQueryList.prototype.media; } \
               catch (error) { return error.message; } })()"
        ),
        json!("Illegal invocation")
    );

    assert_eq!(
        ask(
            &mut realm,
            "matchMedia('(hover: hover)').addListener.toString().indexOf('[native code]') >= 0"
        ),
        json!(true)
    );
}

#[test]
fn the_permissions_surface_is_built_natively_and_keeps_its_identity() {
    let (mut realm, _) = mounted();

    assert_eq!(
        ask(&mut realm, "navigator.permissions === navigator.permissions"),
        json!(true),
        "chrome hands back the same Permissions object every time"
    );

    assert_eq!(
        ask(
            &mut realm,
            "navigator.permissions.query.toString().indexOf('[native code]') >= 0"
        ),
        json!(true)
    );
    assert_eq!(ask(&mut realm, "navigator.permissions.query.name"), json!("query"));
    assert_eq!(ask(&mut realm, "navigator.permissions.query.length"), json!(1));

    assert_eq!(
        ask(
            &mut realm,
            "navigator.permissions.query({ name: 'geolocation' }) instanceof Promise"
        ),
        json!(true)
    );

    assert_eq!(
        ask(
            &mut realm,
            "Object.getOwnPropertyDescriptor(PermissionStatus.prototype, 'state')\
             .get.toString().indexOf('[native code]') >= 0"
        ),
        json!(true)
    );

    realm
        .eval_unit(
            "globalThis.__seen = null; \
             navigator.permissions.query({ name: 'camera' }).then(function (status) { \
               globalThis.__seen = [ \
                 Object.prototype.toString.call(status), \
                 Object.getOwnPropertyNames(status).length, \
                 status instanceof PermissionStatus, \
                 status.name, \
                 status.state \
               ]; \
             });",
            "query",
        )
        .unwrap();

    assert_eq!(
        ask(&mut realm, "__seen"),
        json!(["[object PermissionStatus]", 0, true, "camera", "prompt"])
    );

    assert_eq!(
        ask(
            &mut realm,
            "(function () { try { \
               navigator.permissions.query.call({}, { name: 'camera' }); return 'no throw'; } \
               catch (error) { return error.message; } })()"
        ),
        json!("Illegal invocation")
    );
}

#[test]
fn the_plugin_list_has_the_shape_a_desktop_chrome_reports() {
    let (mut realm, _) = mounted();

    assert_eq!(ask(&mut realm, "navigator.plugins.length"), json!(5));
    assert_eq!(ask(&mut realm, "navigator.plugins[0].name"), json!("PDF Viewer"));
    assert_eq!(
        ask(&mut realm, "navigator.plugins.namedItem('Chrome PDF Viewer').name"),
        json!("Chrome PDF Viewer")
    );
    assert_eq!(
        ask(&mut realm, "navigator.plugins instanceof PluginArray"),
        json!(true)
    );
    assert_eq!(
        ask(&mut realm, "Object.prototype.toString.call(navigator.plugins)"),
        json!("[object PluginArray]")
    );
}

#[test]
fn permissions_query_agrees_with_the_profile() {
    let (mut realm, _) = mounted();

    realm
        .eval_unit(
            "globalThis.__state = null; \
             navigator.permissions.query({ name: 'notifications' }) \
               .then(function (s) { globalThis.__state = s.state; });",
            "query",
        )
        .unwrap();

    assert_eq!(
        ask(&mut realm, "__state"),
        json!(Profile::desktop_chrome().permissions["notifications"])
    );
}

#[test]
fn the_scaffolding_helpers_do_not_survive_installation() {
    let (mut realm, _) = mounted();

    for helper in [
        "__wreDefine",
        "__wreInterface",
        "__wreInstance",
        "__wreWebglParameter",
        "__wreWebglExtensions",
        "__wreCanPlayType",
        "__wreMatchMedia",
        "__wrePermission",
    ] {
        assert_eq!(
            ask(&mut realm, &format!("typeof globalThis.{helper}")),
            json!("undefined"),
            "{helper} was left behind"
        );
    }

    let leaked = ask(
        &mut realm,
        "Object.getOwnPropertyNames(globalThis).filter(function (n) { return n.indexOf('__wre') === 0; })",
    );
    assert_eq!(leaked, json!([]));
}

#[test]
fn the_instrumentation_is_not_reachable_from_the_page() {
    let (mut realm, _) = mounted();

    let found = ask(
        &mut realm,
        r#"(function () {
             var hits = [];
             var names = Object.getOwnPropertyNames(globalThis);
             for (var i = 0; i < names.length; i++) {
               var value;
               try { value = globalThis[names[i]]; } catch (error) { continue; }
               if (!value || typeof value !== "object") continue;
               if (typeof value.push === "function" &&
                   typeof value.drain === "function" &&
                   typeof value.describe === "function") {
                 hits.push(names[i]);
               }
               if (typeof value.watch === "function" && typeof value.trace === "function") {
                 hits.push(names[i]);
               }
             }
             return hits;
           })()"#,
    );

    assert_eq!(found, json!([]), "the instrumentation is reachable from the page");

    for expression in [
        "Object.getOwnPropertyNames(globalThis).filter(function (n) { return n.indexOf('__wre') === 0; })",
        "Object.getOwnPropertyNames(globalThis).filter(function (n) { return /Roles$/.test(n); })",
    ] {
        assert_eq!(ask(&mut realm, expression), json!([]), "{expression}");
    }
}

#[test]
fn the_instrumentation_still_answers_the_rust_side() {
    let (mut realm, _) = mounted();

    realm.eval_unit("console.log('from the page'); setTimeout(function () {}, 5);", "test").unwrap();

    assert_eq!(realm.pending_timers().unwrap(), 1);
    assert_eq!(realm.run_timers(2).unwrap(), 1);

    let records = realm.records().unwrap();
    assert_eq!(records.console.len(), 1);
    assert_eq!(records.console[0].text, "from the page");
    assert!(realm.records().unwrap().console.is_empty(), "drain did not clear");

    assert!(realm.watch("globalThis", "navigator", "navigator").unwrap());
    realm.eval_unit("navigator.userAgent;", "test").unwrap();
    let records = realm.records().unwrap();
    assert!(
        records.access.iter().any(|entry| entry.key == "userAgent"),
        "{:?}",
        records.access
    );
}

#[test]
fn a_changed_profile_changes_what_the_page_sees() {
    let mut profile = Profile::desktop_chrome();
    profile.set("Navigator", "hardwareConcurrency", json!(2)).unwrap();
    profile.set("Navigator", "webdriver", json!(true)).unwrap();

    let mut realm = Realm::new(RealmOptions::default()).unwrap();
    install(&mut realm, &profile).unwrap();

    assert_eq!(ask(&mut realm, "navigator.hardwareConcurrency"), json!(2));
    assert_eq!(ask(&mut realm, "navigator.webdriver"), json!(true));
}

#[test]
fn a_clean_run_records_no_misses() {
    let (mut realm, sandbox) = mounted();

    ask(&mut realm, "navigator.userAgent");
    ask(&mut realm, "screen.height");
    ask(&mut realm, "matchMedia('(pointer: fine)').matches");

    assert!(sandbox.misses().is_empty(), "{:?}", sandbox.misses());
}

#[test]
fn the_installer_reports_what_it_installed() {
    let (_, sandbox) = mounted();

    assert!(sandbox.installed().contains(&"Navigator.userAgent".to_string()));
    assert!(sandbox.installed().contains(&"Screen.availWidth".to_string()));
    assert!(sandbox.installed().len() > 30);
}
