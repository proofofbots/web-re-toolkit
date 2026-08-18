use std::sync::{Arc, Mutex};

use serde_json::{Value, json};

use wre_behavior::stream::{Point, Stream};
use wre_live::realm::RealmOptions;
use wre_sandbox::browser::{Answer, Browser, Held, Hooks, Request, Transport, open};
use wre_sandbox::page::Page;
use wre_sandbox::profile::Profile;

const HTML: &str = r#"<!doctype html><html><head><title>Sign in</title>
<script src="/akam/13/abcdef"></script></head>
<body><form method="post" action="/login">
<input type="hidden" name="__RequestVerificationToken" value="secret">
<input type="email" name="Username" required>
<input type="password" name="Password">
</form></body></html>"#;

#[derive(Default)]
struct Recorder {
    seen: Mutex<Vec<Request>>,
}

impl Transport for Recorder {
    fn send(&self, request: &Request) -> Answer {
        let mut seen = self.seen.lock().unwrap();
        seen.push(request.clone());
        Answer {
            status: 201,
            body: r#"{"success":true}"#.to_string(),
            headers: Vec::new(),
            paced: 0.0,
        }
    }
}

fn mounted() -> Browser {
    let page = Page::read("https://login.example.com/identity/user/login", HTML)
        .with_referrer("https://www.example.com/")
        .with_epoch(1_760_000_000_000.0);

    open(&Profile::desktop_chrome(), &page, Hooks::default(), RealmOptions::default())
        .expect("browser")
}

fn ask(browser: &mut Browser, expression: &str) -> Value {
    browser.eval(expression).expect(expression)
}

#[test]
fn the_document_carries_the_page_it_was_opened_with() {
    let mut browser = mounted();

    assert_eq!(ask(&mut browser, "document.title"), json!("Sign in"));
    assert_eq!(
        ask(&mut browser, "location.href"),
        json!("https://login.example.com/identity/user/login")
    );
    assert_eq!(ask(&mut browser, "location.hostname"), json!("login.example.com"));
    assert_eq!(ask(&mut browser, "document.referrer"), json!("https://www.example.com/"));
    assert_eq!(ask(&mut browser, "document.domain"), json!("login.example.com"));
    assert_eq!(ask(&mut browser, "document.characterSet"), json!("UTF-8"));
    assert_eq!(ask(&mut browser, "document.scripts.length"), json!(1));
    assert_eq!(
        ask(&mut browser, "document.scripts[0].src"),
        json!("https://login.example.com/akam/13/abcdef")
    );
}

#[test]
fn the_form_inventory_is_the_one_the_html_declares() {
    let mut browser = mounted();

    assert_eq!(ask(&mut browser, "document.getElementsByTagName('input').length"), json!(3));
    assert_eq!(ask(&mut browser, "document.forms.length"), json!(1));
    assert_eq!(
        ask(&mut browser, "document.getElementsByTagName('input')[1].getAttribute('name')"),
        json!("Username")
    );
    assert_eq!(
        ask(&mut browser, "document.getElementsByTagName('input')[1].required"),
        json!(true)
    );
    assert_eq!(
        ask(&mut browser, "document.getElementsByTagName('input')[0].offsetParent === null"),
        json!(true)
    );
    assert_eq!(
        ask(&mut browser, "document.querySelectorAll('input[type=password]').length"),
        json!(1)
    );
    assert_eq!(
        ask(&mut browser, "document.getElementsByTagName('input')[1].form === document.forms[0]"),
        json!(true)
    );
}

#[test]
fn a_field_reports_its_value_attribute_as_its_default_value() {
    let mut browser = mounted();

    assert_eq!(
        ask(&mut browser, "document.getElementsByTagName('input')[0].getAttribute('value')"),
        json!("secret")
    );
    assert_eq!(
        ask(&mut browser, "document.getElementsByTagName('input')[0].defaultValue"),
        json!("secret")
    );
    assert_eq!(
        ask(&mut browser, "document.getElementsByTagName('input')[1].defaultValue"),
        json!("")
    );
}

#[test]
fn the_dom_surface_reads_as_native_code() {
    let mut browser = mounted();

    for expression in [
        "document.createElement.toString()",
        "document.querySelectorAll.toString()",
        "XMLHttpRequest.prototype.send.toString()",
        "Element.prototype.getBoundingClientRect.toString()",
        "EventTarget.prototype.addEventListener.toString()",
        "Date.now.toString()",
        "Object.getOwnPropertyDescriptor(Document.prototype, 'cookie').get.toString()",
        "setTimeout.toString()",
    ] {
        let text = ask(&mut browser, expression);
        assert!(
            text.as_str().unwrap_or_default().contains("[native code]"),
            "{expression} reads as {text}"
        );
    }

    assert!(
        ask(&mut browser, "Function.prototype.toString.toString()")
            .as_str()
            .unwrap()
            .contains("[native code]")
    );

    assert_eq!(
        ask(&mut browser, "(function named() { return 1; }).toString()"),
        json!("function named() { return 1; }")
    );
}

#[test]
fn the_dom_prototypes_have_the_shape_a_browser_has() {
    let mut browser = mounted();

    assert_eq!(ask(&mut browser, "document instanceof Document"), json!(true));
    assert_eq!(ask(&mut browser, "document instanceof Node"), json!(true));
    assert_eq!(
        ask(&mut browser, "document.createElement('canvas') instanceof HTMLCanvasElement"),
        json!(true)
    );
    assert_eq!(
        ask(&mut browser, "document.body instanceof HTMLElement"),
        json!(true)
    );
    assert_eq!(
        ask(&mut browser, "Object.prototype.toString.call(document)"),
        json!("[object HTMLDocument]")
    );
    assert_eq!(
        ask(&mut browser, "Object.getPrototypeOf(HTMLElement.prototype) === Element.prototype"),
        json!(true)
    );
    assert_eq!(
        ask(&mut browser, "typeof XPathResult !== 'undefined' && typeof FileReader !== 'undefined'"),
        json!(true)
    );
    assert_eq!(
        ask(
            &mut browser,
            "['PointerEvent','TouchEvent','DeviceOrientationEvent','DeviceMotionEvent','PushManager']\
             .every(function (name) { return typeof globalThis[name] === 'function'; })"
        ),
        json!(true)
    );
    assert_eq!(
        ask(&mut browser, "Document.prototype.hasOwnProperty('hasPrivateToken')"),
        json!(true)
    );
}

#[test]
fn the_clock_runs_forward_and_timers_fire_in_order() {
    let mut browser = mounted();

    browser
        .run(
            "globalThis.marks = []; \
             setTimeout(function () { marks.push(['late', Date.now()]); }, 500); \
             setTimeout(function () { marks.push(['early', Date.now()]); }, 100); \
             var ticks = 0; \
             var id = setInterval(function () { ticks += 1; if (ticks > 2) clearInterval(id); }, 50);",
            "test:timers",
        )
        .unwrap();

    assert_eq!(ask(&mut browser, "marks.length"), json!(0));

    let ran = browser.advance(1000.0).unwrap();
    assert!(ran >= 3, "{ran} timers ran");

    assert_eq!(ask(&mut browser, "marks.map(function (m) { return m[0]; })"), json!(["early", "late"]));
    assert_eq!(ask(&mut browser, "ticks"), json!(3));

    let early = ask(&mut browser, "marks[0][1]").as_f64().unwrap();
    let late = ask(&mut browser, "marks[1][1]").as_f64().unwrap();

    assert!(early >= 1_760_000_000_100.0, "{early}");
    assert!(late >= early + 380.0, "early {early} late {late}");
    assert!(browser.now().unwrap() >= 1_760_000_001_000.0);
    assert!(browser.elapsed().unwrap() >= 1000.0);
}

#[test]
fn a_listener_hears_the_events_the_host_fires() {
    let mut browser = mounted();

    browser
        .run(
            "globalThis.heard = []; \
             document.addEventListener('mousemove', function (event) { \
               heard.push([event.type, event.clientX, event.isTrusted, event.constructor.name]); \
             }); \
             document.addEventListener('keydown', function (event) { heard.push([event.type, event.key, event.keyCode]); }); \
             globalThis.addEventListener('load', function () { heard.push(['load']); });",
            "test:listeners",
        )
        .unwrap();

    browser.fire("mousemove", json!({ "clientX": 410, "clientY": 220 })).unwrap();
    browser.fire("keydown", json!({ "key": "a" })).unwrap();
    browser.load().unwrap();

    assert_eq!(
        ask(&mut browser, "heard[0]"),
        json!(["mousemove", 410, true, "MouseEvent"])
    );
    assert_eq!(ask(&mut browser, "heard[1]"), json!(["keydown", "a", 65]));
    assert_eq!(ask(&mut browser, "heard[2]"), json!(["load"]));
    assert_eq!(ask(&mut browser, "document.readyState"), json!("complete"));
}

#[test]
fn a_behaviour_stream_arrives_as_pointer_and_mouse_events() {
    let mut browser = mounted();

    browser
        .run(
            "globalThis.seen = {}; \
             ['pointermove','mousemove','pointerdown','mousedown','pointerup','mouseup','click','keydown'] \
               .forEach(function (name) { \
                 document.addEventListener(name, function () { seen[name] = (seen[name] || 0) + 1; }); \
               });",
            "test:behaviour",
        )
        .unwrap();

    let mut stream = Stream::seeded(7);
    stream.move_to(Point::new(420.0, 260.0)).unwrap();
    stream.click();
    stream.type_text("hi");

    let fired = browser.play(stream.events()).unwrap();

    assert!(fired > 10, "{fired} events fired");
    assert!(ask(&mut browser, "seen.pointermove").as_u64().unwrap() > 3);
    assert_eq!(ask(&mut browser, "seen.mousemove"), ask(&mut browser, "seen.pointermove"));
    assert_eq!(ask(&mut browser, "seen.click"), json!(1));
    assert_eq!(ask(&mut browser, "seen.keydown"), json!(2));
}

#[test]
fn a_post_from_the_page_reaches_the_transport_and_its_answer_comes_back() {
    let page = Page::read("https://www.example.com/tracking", HTML);
    let recorder = Arc::new(Recorder::default());
    let hooks = Hooks {
        transport: Arc::clone(&recorder) as Arc<dyn Transport>,
        cookies: Arc::new(Held::seeded("bm_sz=aaa; _abck=bbb~-1~ccc")),
    };

    let mut browser = open(
        &Profile::desktop_chrome(),
        &page,
        hooks,
        RealmOptions::default(),
    )
    .expect("browser");

    browser
        .run(
            "var request = new XMLHttpRequest(); \
             request.open('POST', 'https://www.example.com/akam/13/abcdef'); \
             request.setRequestHeader('Content-Type', 'application/json'); \
             request.send(JSON.stringify({ sensor_data: '7a74G7m23Vrp' })); \
             globalThis.answer = [request.status, request.responseText, request.readyState];",
            "test:xhr",
        )
        .unwrap();

    assert_eq!(
        ask(&mut browser, "answer"),
        json!([201, r#"{"success":true}"#, 4])
    );

    let seen = recorder.seen.lock().unwrap().clone();
    assert_eq!(seen.len(), 1);
    assert_eq!(seen[0].method, "POST");
    assert_eq!(seen[0].url, "https://www.example.com/akam/13/abcdef");
    assert_eq!(seen[0].headers.get("content-type").map(String::as_str), Some("application/json"));
    assert!(seen[0].body.as_deref().unwrap().contains("sensor_data"));

    let recorded = browser.requests();
    assert_eq!(recorded.len(), 1);
    assert_eq!(recorded[0].source, "xhr");
}

#[test]
fn document_cookie_reads_and_writes_through_the_host() {
    let page = Page::read("https://www.example.com/", HTML);
    let hooks = Hooks {
        transport: Arc::new(wre_sandbox::browser::Offline),
        cookies: Arc::new(Held::seeded("bm_sz=aaa; _abck=bbb~-1~ccc")),
    };

    let mut browser =
        open(&Profile::desktop_chrome(), &page, hooks, RealmOptions::default()).expect("browser");

    assert_eq!(ask(&mut browser, "document.cookie"), json!("bm_sz=aaa; _abck=bbb~-1~ccc"));

    browser
        .run("document.cookie = 'ak_bmsc=fresh; path=/; secure';", "test:cookie")
        .unwrap();

    assert_eq!(
        ask(&mut browser, "document.cookie"),
        json!("bm_sz=aaa; _abck=bbb~-1~ccc; ak_bmsc=fresh")
    );
    assert_eq!(browser.cookies().unwrap(), "bm_sz=aaa; _abck=bbb~-1~ccc; ak_bmsc=fresh");
}

#[test]
fn the_canvas_answers_from_the_profile_and_records_what_it_has_not_seen() {
    let mut profile = Profile::desktop_chrome();
    profile
        .canvas
        .insert("default".to_string(), "data:image/png;base64,RECORDED".to_string());

    let page = Page::new("https://www.example.com/");
    let mut browser =
        open(&profile, &page, Hooks::default(), RealmOptions::default()).expect("browser");

    let url = ask(
        &mut browser,
        "(function () { \
           var canvas = document.createElement('canvas'); \
           canvas.width = 280; canvas.height = 60; \
           var context = canvas.getContext('2d'); \
           context.textBaseline = 'top'; \
           context.font = '14px Arial'; \
           context.fillText('wre', 2, 15); \
           return canvas.toDataURL(); \
         })()",
    );

    assert_eq!(url, json!("data:image/png;base64,RECORDED"));
    assert!(
        browser.misses().iter().any(|entry| entry.starts_with("canvas toDataURL(")),
        "{:?}",
        browser.misses()
    );
}

#[test]
fn measure_text_scales_the_recorded_font_widths() {
    let mut browser = mounted();

    let arial = ask(
        &mut browser,
        "(function () { \
           var context = document.createElement('canvas').getContext('2d'); \
           context.font = '72px Arial'; \
           return context.measureText('mmmmmmmmmmlli').width; \
         })()",
    )
    .as_f64()
    .unwrap();

    let profile = Profile::desktop_chrome();
    let recorded = profile.font_widths["Arial"];

    assert!((arial - recorded).abs() < 0.01, "{arial}");

    let half = ask(
        &mut browser,
        "(function () { \
           var context = document.createElement('canvas').getContext('2d'); \
           context.font = '36px \"Courier New\"'; \
           return context.measureText('mmmmmmmmmmlli').width; \
         })()",
    )
    .as_f64()
    .unwrap();

    let courier = profile.font_widths["Courier New"] / 2.0;

    assert!((half - courier).abs() < 0.01, "{half}");
}

#[test]
fn the_profile_sections_reach_the_page() {
    let mut browser = mounted();

    assert_eq!(ask(&mut browser, "navigator.userAgentData.platform"), json!("macOS"));
    assert_eq!(ask(&mut browser, "navigator.connection.effectiveType"), json!("4g"));
    assert_eq!(ask(&mut browser, "navigator.mimeTypes.length"), json!(2));
    assert_eq!(
        ask(&mut browser, "navigator.mimeTypes['application/pdf'].suffixes"),
        json!("pdf")
    );
    assert_eq!(ask(&mut browser, "navigator.plugins.length"), json!(5));
    assert_eq!(
        ask(&mut browser, "navigator.plugins['Chrome PDF Viewer'].length"),
        json!(2)
    );
    assert_eq!(ask(&mut browser, "typeof navigator.javaEnabled"), json!("function"));
    assert_eq!(ask(&mut browser, "navigator.javaEnabled()"), json!(false));
    assert_eq!(ask(&mut browser, "navigator.getGamepads().length"), json!(4));
    let profile = Profile::desktop_chrome();
    let intl = profile.intl.clone().expect("the profile carries an intl");

    assert_eq!(
        ask(&mut browser, "performance.memory.jsHeapSizeLimit"),
        json!(profile.memory.expect("the profile carries a heap").js_heap_size_limit as u64)
    );
    assert_eq!(
        ask(&mut browser, "Intl.DateTimeFormat().resolvedOptions().timeZone"),
        json!(intl.time_zone)
    );
    assert_eq!(
        ask(&mut browser, "new Date().getTimezoneOffset()"),
        json!(intl.timezone_offset)
    );
    assert_eq!(ask(&mut browser, "screen.orientation.type"), json!("landscape-primary"));
    assert_eq!(ask(&mut browser, "typeof chrome.loadTimes"), json!("function"));
    assert_eq!(ask(&mut browser, "chrome.loadTimes().npnNegotiatedProtocol"), json!("h2"));
    assert_eq!(ask(&mut browser, "matchMedia('(pointer: fine)').matches"), json!(true));
    assert_eq!(ask(&mut browser, "typeof AudioContext"), json!("function"));
    assert_eq!(
        ask(&mut browser, "new AudioContext().sampleRate"),
        json!(Profile::desktop_chrome().audio.expect("the profile carries audio").sample_rate as u64)
    );
}

#[test]
fn storage_and_codecs_behave() {
    let mut browser = mounted();

    assert_eq!(
        ask(
            &mut browser,
            "(function () { \
               localStorage.setItem('dummy', 'test'); \
               var read = localStorage.getItem('dummy'); \
               localStorage.removeItem('dummy'); \
               return [read, localStorage.length, sessionStorage.getItem('nothing')]; \
             })()"
        ),
        json!(["test", 0, null])
    );

    assert_eq!(ask(&mut browser, "btoa('sensor')"), json!("c2Vuc29y"));
    assert_eq!(ask(&mut browser, "atob('c2Vuc29y')"), json!("sensor"));
    assert_eq!(
        ask(&mut browser, "new TextDecoder().decode(new TextEncoder().encode('héllo'))"),
        json!("héllo")
    );
    assert_eq!(
        ask(&mut browser, "new URL('/a/b?x=1', 'https://www.example.com/deep/').pathname"),
        json!("/a/b")
    );
    assert_eq!(
        ask(&mut browser, "new URL('https://a.test:8443/p?q=2#h').searchParams.get('q')"),
        json!("2")
    );
    assert_eq!(
        ask(&mut browser, "new URLSearchParams({ a: '1', b: '2' }).toString()"),
        json!("a=1&b=2")
    );
}

#[test]
fn an_init_charge_moves_the_clock_the_first_time_a_field_is_written() {
    let mut browser = mounted();

    browser.charge_on("bmak", "startTs", 25.0).unwrap();

    let before = browser.elapsed().unwrap();
    browser
        .run(
            "globalThis.bmak = { startTs: 0 }; bmak.startTs = Date.now(); \
             globalThis.after = Date.now() - bmak.startTs;",
            "test:charge",
        )
        .unwrap();

    let after = browser.elapsed().unwrap();

    assert!(after - before >= 25.0, "before {before} after {after}");
    assert_eq!(ask(&mut browser, "typeof bmak.startTs"), json!("number"));

    browser
        .run("bmak.startTs = Date.now(); bmak.startTs = Date.now();", "test:charge-again")
        .unwrap();

    let again = browser.elapsed().unwrap();
    assert!(again - after < 25.0, "the charge was applied twice: {after} then {again}");
}

#[test]
fn the_page_cannot_see_the_host_bridges() {
    let mut browser = mounted();

    let leaks = ask(
        &mut browser,
        "Object.getOwnPropertyNames(globalThis).filter(function (name) { \
           return name.indexOf('__wre') === 0; \
         })",
    );

    assert_eq!(leaks, json!([]));
}

#[test]
fn a_shared_worker_answers_the_page_with_what_it_measured() {
    let mut browser = mounted();

    let answer = ask(
        &mut browser,
        r#"(function () {
             var source = "onconnect = function (event) { event.ports[0].postMessage({ \
                 ua: navigator.userAgent, \
                 tz: Intl.DateTimeFormat().resolvedOptions().timeZone, \
                 hc: navigator.hardwareConcurrency, \
                 gpu: (function () { \
                   var context = new OffscreenCanvas(0, 0).getContext('webgl'); \
                   var info = context.getExtension('WEBGL_debug_renderer_info'); \
                   return context.getParameter(info.UNMASKED_RENDERER_WEBGL); \
                 })(), \
                 gpu2: (function () { \
                   var context = new OffscreenCanvas(0, 0).getContext('webgl2'); \
                   var info = context.getExtension('WEBGL_debug_renderer_info'); \
                   return context.getParameter(info.UNMASKED_VENDOR_WEBGL); \
                 })() \
               }); };";

             var url = URL.createObjectURL(new Blob([source], { type: "application/javascript" }));
             var worker = new SharedWorker(url);
             var seen = null;

             worker.port.onmessage = function (event) { seen = event.data; };
             worker.port.start();

             return seen;
           })()"#,
    );

    assert_eq!(answer["ua"], ask(&mut browser, "navigator.userAgent"));
    assert_eq!(answer["hc"], ask(&mut browser, "navigator.hardwareConcurrency"));
    assert!(answer["gpu"].is_string(), "no webgl renderer in the worker: {answer}");
    assert!(answer["gpu2"].is_string(), "no webgl2 vendor in the worker: {answer}");
}

#[test]
fn the_worker_cannot_reach_the_extensions_it_probes() {
    let mut browser = mounted();

    browser
        .run(
            r#"globalThis.__reached = null;
               fetch("chrome-extension://aeblfdkhhhdcdjpifhhbdiojplfjncoa/x.png", { method: "HEAD" })
                 .then(function () { globalThis.__reached = true; })
                 .catch(function () { globalThis.__reached = false; });"#,
            "test:extension-probe",
        )
        .expect("probe");

    assert_eq!(ask(&mut browser, "globalThis.__reached"), json!(false));
}

#[test]
fn the_clocks_agree_with_each_other() {
    let mut browser = mounted();

    let drift = ask(
        &mut browser,
        "Math.round(Date.now() - (performance.timeOrigin + performance.now()))",
    );

    let start = ask(&mut browser, "performance.now()").as_f64().unwrap_or_default();
    let stamp = ask(&mut browser, "Date.now()").as_f64().unwrap_or_default();
    let later = ask(&mut browser, "performance.now()").as_f64().unwrap_or_default();
    let after = ask(&mut browser, "Date.now()").as_f64().unwrap_or_default();

    assert!(later >= start, "performance.now went backwards: {start} then {later}");
    assert!(after >= stamp, "Date.now went backwards: {stamp} then {after}");
    assert!(
        drift.as_f64().unwrap_or_default().abs() < 50.0,
        "Date.now and performance disagree by {drift} ms"
    );
}

#[test]
fn every_permission_the_page_asks_about_settles() {
    let mut browser = mounted();

    browser
        .run(
            r#"globalThis.__states = [];
               var names = ["geolocation", "notifications", "push", "midi", "camera", "microphone",
                 "speaker", "device-info", "background-sync", "bluetooth", "persistent-storage",
                 "ambient-light-sensor", "accelerometer", "gyroscope", "magnetometer",
                 "clipboard-read", "clipboard-write", "payment-handler", "idle-detection",
                 "screen-wake-lock"];

               names.forEach(function (name) {
                 try {
                   navigator.permissions.query({ name: name })
                     .then(function (status) { globalThis.__states.push(name + ":" + status.state); })
                     .catch(function () { globalThis.__states.push(name + ":error"); });
                 } catch (error) {
                   globalThis.__states.push(name + ":threw");
                 }
               });"#,
            "test:permissions",
        )
        .expect("permissions");

    let settled = ask(&mut browser, "globalThis.__states.length");
    assert_eq!(settled, json!(20), "not every permission settled: {:?}", ask(&mut browser, "globalThis.__states"));
}

#[test]
fn the_navigator_keeps_its_prototype_and_its_permissions() {
    let mut browser = mounted();

    assert_eq!(
        ask(&mut browser, "Object.getPrototypeOf(navigator) === Navigator.prototype"),
        json!(true),
        "navigator is not a Navigator"
    );

    assert_eq!(ask(&mut browser, "Boolean(navigator.permissions)"), json!(true));
    assert_eq!(ask(&mut browser, "typeof navigator.permissions.query"), json!("function"));
    assert_eq!(
        ask(&mut browser, "Object.prototype.hasOwnProperty('javaEnabled')"),
        json!(false),
        "the navigator methods landed on Object.prototype"
    );
    assert_eq!(
        ask(&mut browser, "Navigator.prototype.hasOwnProperty('javaEnabled')"),
        json!(true)
    );
}

#[test]
fn an_invalid_permission_name_rejects_instead_of_throwing() {
    let mut browser = mounted();

    browser
        .run(
            r#"globalThis.__answer = "not settled";
               try {
                 navigator.permissions.query({ name: "speaker" })
                   .then(function (status) { globalThis.__answer = "resolved:" + status.state; })
                   .catch(function (error) { globalThis.__answer = "rejected"; });
               } catch (error) {
                 globalThis.__answer = "threw";
               }"#,
            "test:invalid-permission",
        )
        .expect("query");

    assert_eq!(ask(&mut browser, "globalThis.__answer"), json!("rejected"));
}

#[test]
fn the_permission_sweep_the_sensor_runs_finishes() {
    let mut browser = mounted();

    browser
        .run(
            r#"globalThis.__digits = "";
               var names = ["geolocation", "notifications", "push", "midi", "camera", "microphone",
                 "speaker", "device-info", "background-sync", "bluetooth", "persistent-storage",
                 "ambient-light-sensor", "accelerometer", "gyroscope", "magnetometer",
                 "clipboard", "accessibility-events", "payment-handler", "idle-detection",
                 "screen-wake-lock"];

               var states = [];
               var pending = names.map(function (name, index) {
                 return navigator.permissions.query({ name: name })
                   .then(function (status) { states[index] = status.state === "granted" ? 9 : 4; })
                   .catch(function () { states[index] = 2; });
               });

               Promise.all(pending).then(function () { globalThis.__digits = states.join(""); });"#,
            "test:permission-sweep",
        )
        .expect("sweep");

    let digits = ask(&mut browser, "globalThis.__digits");
    assert_eq!(digits.as_str().map(str::len), Some(20), "sweep did not finish: {digits}");
}

#[test]
fn the_last_permission_in_the_sweep_settles_too() {
    let mut browser = mounted();

    browser
        .run(
            r#"globalThis.__last = "not settled";
               navigator.permissions.query({ name: "accessibility-events" })
                 .then(function (status) { globalThis.__last = "resolved:" + status.state; })
                 .catch(function () { globalThis.__last = "rejected"; });"#,
            "test:last-permission",
        )
        .expect("query");

    let answer = ask(&mut browser, "globalThis.__last");
    assert_ne!(answer, json!("not settled"), "accessibility-events never settled");
}
