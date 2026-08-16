---
title: The browser surface
description: wre-sandbox installs a browser surface into a V8 realm as native bindings, with values captured from real devices.
---

`wre-sandbox` installs a browser surface into a `wre-live` realm. The values come from a profile captured off a real device. The object shapes come from V8 itself.

That second part is the whole point. A fake browser written in JavaScript has to lie about what it is, and the lie is checkable: a getter defined as a JavaScript function reports its own source from `Function.prototype.toString`, so every JavaScript-implemented surface needs a global `toString` patch to hide itself, and that patch is then the thing worth detecting. One of the protections we have read checks `toString` behaviour thirty four times.

A getter created from a V8 function template does not need hiding. It reports `function () { [native code] }` because it genuinely is native code.

```
wre sandbox check
```

```
| check                                          | verdict |
| getters are native                             | holds   |
| toString is untouched                          | holds   |
| wrong receiver throws                          | holds   |
| the brand tag is right                         | holds   |
| properties sit on the prototype                | holds   |
| no fixed toolkit prefix is reachable           | holds   |
| the instrumentation is off the global          | holds   |
| matchMedia is native and so is what it returns | holds   |
| permissions keeps its identity                 | holds   |
```

## Profiles

A profile is one real device's readings. They live in `profiles/` as one JSON file per device, and every sandbox command takes `--profile <id>`:

```
wre sandbox list
wre sandbox profile --profile macos-chrome-2026-08-16
wre sandbox check --all
wre sandbox check --random
```

With no flag the first profile in the directory is used, and if the directory is empty the built-in one is. The built-in is a real capture from a MacBook Pro M1 Pro running Chrome 140, compiled in as `builtin-desktop-chrome` so `wre sandbox check` runs on a fresh clone. It is listed as built in, never written to disk, and cannot be overwritten by a capture.

A target can still carry its own profile in `targets/<name>.toml` under `[sandbox]`; `--target <name>` reads that instead of the library.

Nothing here generates values. There is no synthetic profile, no randomised user agent, no invented GPU string. If a device is not in the library, capture it.

## Capturing a device

```
wre sandbox capture
```

That serves a page on `http://127.0.0.1:8099` and waits. Open it in the browser you want to replay, name the device, press **Send to wre**, and the profile lands in `profiles/`. The process exits after one capture; `--keep` leaves it up for several devices in a row.

To capture a phone or another machine, bind wider:

```
wre sandbox capture --host 0.0.0.0 --keep
```

Two things change off loopback. The page and the profile travel over the LAN unencrypted, and `crypto.subtle` is unavailable outside a secure context, so the canvas hashes fall back to a 32 bit FNV-1a and are marked `fnv1a:` instead of `sha256:`. The command says so when it binds.

The page also has a **Download JSON** button for a browser that cannot reach the machine running `wre`. Store that file with:

```
wre sandbox import ~/Downloads/sandbox-profile.json --id pixel-8-chrome
```

`--open` skips the manual step: it launches Chrome through `wre-cdp`, opens the page itself and stores what comes back.

```
wre sandbox capture --open --label "MacBook Pro M3, Chrome 151"
```

The page reads: the `Navigator`, `Screen` and `Window` properties the sandbox installs, `navigator.userAgentData` including the high entropy values, `navigator.connection`, the plugin and MIME type lists, WebGL and WebGL2 parameters and extensions from real contexts, `canPlayType` answers for sixteen media types, twenty media queries, twenty five permission states including the names Chrome rejects as invalid, five canvas drawings kept as their real data URLs, a WebGL render, the `AudioContext` numbers and an `OfflineAudioContext` render hash, `getBattery`, the speech synthesis voices, `enumerateDevices`, `performance.memory`, the resolved `Intl` options and timezone offset, `screen.orientation`, the shape of `window.chrome`, the constant `document` properties, `measureText` widths for twenty font families at `72px mmmmmmmmmmlli`, a handful of layout measurements, and the full `Object.getOwnPropertyNames(window)` order.

Canvas images are stored under a digest of the drawing operations that produced them, so a target that replays a known probe gets that device's real pixels back and anything else falls through to `default` and records a miss.

## What a capture is checked against

Every capture is audited on the way in, and `wre sandbox list` shows the warning count per profile. The audit warns about `navigator.webdriver`, a `HeadlessChrome` user agent, a SwiftShader or llvmpipe renderer, a desktop Chrome with no plugins, a platform that disagrees with the user agent, a mobile user agent with no touch points, and geometry that cannot happen (`innerHeight > outerHeight > screen.height`). Empty tables are noted rather than warned about, since the sandbox will record them as misses at replay time.

The audit never refuses a capture. It prints what it found and writes the file, because a headless profile is worth having on purpose as long as you know that is what it is.

## What the shapes get right

**`Function.prototype.toString` is untouched by the install.** Nothing in `install` replaces it, and `wre sandbox check` asserts that it still reports native and is still writable. Mounting a page does replace it with a native equivalent, covered below.

**Accessors are accessors.** `Object.getOwnPropertyDescriptor(Navigator.prototype, 'userAgent')` returns a descriptor with a native `get`, no `set`, and no `value`, which is what Chrome returns. A surface built by assigning plain values would return a data descriptor instead, and the difference is one property read away.

**Properties live on the prototype.** `Object.getOwnPropertyDescriptor(navigator, 'userAgent')` is `undefined`, as in a real browser. The value is reached through `Navigator.prototype`.

**Wrong receivers throw.** Native accessors are registered with a brand. The trampoline walks the receiver's prototype chain looking for that brand behind a V8 private symbol, and throws `TypeError: Illegal invocation` when it is missing. Calling `descriptor.get.call({})` fails the way it fails in Chrome, and the brand itself is not reachable from JavaScript.

**Constructors refuse to run.** `new Navigator()` throws `TypeError: Illegal constructor`.

**Tags are set.** `Object.prototype.toString.call(navigator)` gives `[object Navigator]`, and the same holds for `PluginArray`, `MediaQueryList`, `PermissionStatus` and the rest, because every synthesised interface gets `Symbol.toStringTag`.

**`Window` inherits `EventTarget`.** `globalThis instanceof Window` holds and the prototype chain has the shape a page has.

## matchMedia and permissions

These two used to be native functions wrapping JavaScript object construction: the host call returned data and a JavaScript wrapper built the result with `Object.create`. The wrapper was the tell.

Both are now built in Rust. `matchMedia` is a V8 function with the name `matchMedia` and length 1, and the `MediaQueryList` it returns is created by the host trampoline: prototype set to `MediaQueryList.prototype`, the brand and the query answer stored behind V8 private symbols, and no own properties at all. `media` and `matches` are native accessors on the prototype that read that private state, so `Object.getOwnPropertyNames(mql)` is empty and `MediaQueryList.prototype.media` throws `Illegal invocation`, both as in Chrome.

`navigator.permissions` is a native getter that returns the same `Permissions` object on every read, so `navigator.permissions === navigator.permissions` holds. `query` is a native function returning a real V8 promise resolved with a natively built `PermissionStatus`, whose `name` and `state` are prototype accessors over private state. Calling `navigator.permissions.query.call({}, spec)` throws `Illegal invocation`.

`EventTarget`'s `addEventListener`, `removeEventListener` and `dispatchEvent`, and `MediaQueryList`'s `addListener` and `removeListener`, are native no-ops rather than JavaScript stubs, so their `toString` matches the rest.

The plugin list and the WebGL context are still assembled in JavaScript. `navigator.plugins` returns a cached list built with `Object.create`, and `WebGLRenderingContext.prototype.getExtension` is a JavaScript function. Those are the next ones to move.

## The realm's own instrumentation

`wre-live` keeps a console buffer, a timer queue and the access traps in one object. It used to be a global: first `__wre`, then a fresh nine letter name per realm. Either way a script could find it by enumerating the global and looking for an object with a `push`, `drain` and `describe` triple.

That object is no longer on the global. The prelude is one closure that returns the control object as the script's completion value, and the Rust side keeps it as a `v8::Global` handle. `Realm::records`, `run_timers`, `pending_timers`, `watch` and `trace` call methods on that handle directly through the V8 API, never by evaluating a name. From inside the realm there is no reference to reach: `Object.getOwnPropertyNames(globalThis)` shows `console`, `setTimeout` and the rest of the surface, and nothing else.

`wre sandbox check` asserts it, scanning every global for an object carrying `drain` and `push`.

What is left reachable is the behaviour, not the object. `console.log` is a JavaScript function whose source a script can read, `setTimeout` never fires on its own because time only moves when `run_timers` is called, and an access trap replaces the watched property with a `Proxy` that `Object.getOwnPropertyDescriptor` will report. A script that looks for those still finds them. This closes the handle, not the whole class.

Mounted targets get the same treatment. `wre-live`'s mount plants captured roles on a global named freshly per mount, and deletes it as soon as the roles are captured, so the sink is gone before anything else runs.

## Replay and misses

Canvas, WebGL, media and media query answers come from tables in the profile rather than from anything computed. A lookup with no recorded answer returns a neutral value **and records a miss**:

```rust
let sandbox = install(&mut realm, &profile)?;
// ... run the target ...
for miss in sandbox.misses() {
    println!("{miss}");
}
```

Misses are the honest part of the design. A surface that quietly invents an answer produces a payload that looks complete and grades badly for reasons you cannot see. `wre grade` and the miss log are meant to be read together.

## The page

`wre_sandbox::browser::open` mounts a document on top of the installed profile and hands back a `Browser`:

```rust
let page = Page::read(url, html).with_epoch(now_ms());
let mut browser = open(&profile, &page, hooks, RealmOptions::default())?;

browser.charge_on("bmak", "startTs", 25.0)?;
browser.run(&script, "target:sensor")?;
browser.load()?;
browser.play(stream.events())?;
browser.advance(4_000.0)?;
```

`Page::read` takes the page's own HTML and pulls the script list, the title, the forms and every `input`, `textarea` and `select` with its attributes, label count and visibility, dropping `value`. A sensor that walks `document.getElementsByTagName("input")` finds the fields the page actually declares.

What the document carries: `Node`, `Element`, `HTMLElement` and the element subclasses, a tree with the usual mutation methods, attributes, `classList`, `style`, a small selector engine behind `querySelector`, `getElementsByTagName` and `getElementsByClassName`, `getBoundingClientRect`, the event constructors (`Event`, `UIEvent`, `MouseEvent`, `PointerEvent`, `WheelEvent`, `KeyboardEvent`, `TouchEvent`, `Touch`, `CustomEvent`, `DeviceOrientationEvent`, `DeviceMotionEvent`, `ProgressEvent`, `MessageEvent`, `ErrorEvent`), working `EventTarget` methods, `Storage`, `Location`, `History`, `Crypto`, `Performance` with `timing`, `navigation` and `memory`, `XMLHttpRequest`, `fetch`, `Headers`, `Response`, `Blob`, `FormData`, `FileReader`, `TextEncoder`, `TextDecoder`, `URL`, `URLSearchParams`, the observers, `WebSocket`, `RTCPeerConnection`, `Worker`, `PushManager`, `XPathResult`, `AudioContext` and `OfflineAudioContext` answering from the profile, `speechSynthesis`, `Notification`, `Image`, `visualViewport`, `indexedDB` and `window.chrome`.

Time is virtual. `Date`, `performance.now` and the timer queue all read one clock that starts at the epoch the page was opened with and moves when `advance(ms)` is called, plus however long the host has really spent. `charge_on(global, property, ms)` adds a one-off cost the first time a script writes a field, which is how the initialisation time a real browser spends before its first payload gets paid without pretending to do the work.

Requests and cookies leave the realm through the host. `XMLHttpRequest`, `fetch` and `sendBeacon` hand a record to a `Transport`, which can answer offline or send the request for real, and every request is kept. `document.cookie` reads and writes a `CookieStore`, so a jar shared with the HTTP client keeps the page and the transport telling one story. HttpOnly cookies stay out of the script's view.

Canvas is recorded, not drawn. A 2D context logs the operations it is given; `toDataURL` digests that log and asks the profile for the image that device produced for the same drawing. `measureText` scales the recorded font widths. Unknown drawings and unknown fonts record misses.

## The source mask

The DOM is JavaScript, which brings back the problem native accessors avoided: `document.createElement.toString()` would print its source. `Realm::install_source_mask` replaces `Function.prototype.toString` with a native V8 function, and `mask_sources(expression)` marks the members of an object as native behind a V8 private symbol. A marked function reports `function name() { [native code] }`; everything else, including the page's own functions, goes to the original implementation, so a script still reads its own source back verbatim.

The replacement is itself a native function, so `Function.prototype.toString.toString()` reads native, and it stays writable and non-enumerable as in Chrome. This is a narrower lie than a JavaScript `toString` patch, not the absence of one: a script that compares `Function.prototype.toString` against a reference from another realm can still tell.

The profile surface does not need it. Accessors installed from the profile are genuine V8 functions.
