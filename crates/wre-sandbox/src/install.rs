use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};

use serde_json::{Value, json};

use wre_core::error::{Error, Result};
use wre_live::realm::{HostSpec, Realm, Shape};

use crate::profile::Profile;

#[derive(Debug, Clone, Default)]
pub struct Misses {
    entries: Arc<Mutex<Vec<String>>>,
}

impl Misses {
    pub fn record(&self, what: &str) {
        if let Ok(mut entries) = self.entries.lock()
            && entries.len() < 4096
            && !entries.iter().any(|entry| entry == what)
        {
            entries.push(what.to_string());
        }
    }

    pub fn all(&self) -> Vec<String> {
        self.entries.lock().map(|entries| entries.clone()).unwrap_or_default()
    }

    pub fn is_empty(&self) -> bool {
        self.all().is_empty()
    }
}

const SCAFFOLD: &str = r#"
globalThis.__wreDefine = function (holder, name, getter) {
  Object.defineProperty(holder, name, {
    get: getter,
    set: undefined,
    enumerable: true,
    configurable: true
  });
};

globalThis.__wreTag = function (Ctor, name) {
  Object.defineProperty(Ctor, "name", { value: name, configurable: true });
  Object.defineProperty(Ctor.prototype, "constructor", {
    value: Ctor,
    writable: true,
    configurable: true
  });
  Object.defineProperty(Ctor.prototype, Symbol.toStringTag, {
    value: name,
    configurable: true
  });
  globalThis[name] = Ctor;
  return Ctor;
};

globalThis.__wreInterface = function (constructorName, parentName) {
  var Ctor = function () {
    throw new TypeError("Illegal constructor");
  };

  Object.defineProperty(Ctor, "name", { value: constructorName, configurable: true });

  if (parentName && globalThis[parentName]) {
    Object.setPrototypeOf(Ctor.prototype, globalThis[parentName].prototype);
    Object.setPrototypeOf(Ctor, globalThis[parentName]);
  }

  Object.defineProperty(Ctor.prototype, "constructor", {
    value: Ctor,
    writable: true,
    configurable: true
  });

  Object.defineProperty(Ctor.prototype, Symbol.toStringTag, {
    value: constructorName,
    configurable: true
  });

  globalThis[constructorName] = Ctor;
  return Ctor;
};

globalThis.__wreInstance = function (constructorName, instanceName) {
  var Ctor = globalThis[constructorName];

  if (instanceName === "globalThis") {
    Object.setPrototypeOf(globalThis, Ctor.prototype);
    return globalThis;
  }

  var instance = Object.create(Ctor.prototype);
  Object.defineProperty(globalThis, instanceName, {
    value: instance,
    writable: false,
    enumerable: true,
    configurable: true
  });
  return instance;
};
"#;

const EVENT_TARGET: &str = r#"
(function () {
  var EventTarget = function () {};
  Object.defineProperty(EventTarget, "name", { value: "EventTarget", configurable: true });
  Object.defineProperty(EventTarget.prototype, Symbol.toStringTag, {
    value: "EventTarget",
    configurable: true
  });
  globalThis.EventTarget = EventTarget;
})();
"#;

const EVENT_TARGET_METHODS: &str = r#"
(function () {
  EventTarget.prototype.addEventListener = __wreAddListener;
  EventTarget.prototype.removeEventListener = __wreRemoveListener;
  EventTarget.prototype.dispatchEvent = __wreDispatchEvent;

  delete globalThis.__wreAddListener;
  delete globalThis.__wreRemoveListener;
  delete globalThis.__wreDispatchEvent;
})();
"#;

pub struct Sandbox {
    misses: Misses,
    installed: Vec<String>,
}

impl Sandbox {
    pub fn misses(&self) -> Vec<String> {
        self.misses.all()
    }

    pub fn installed(&self) -> &[String] {
        &self.installed
    }
}

pub fn install(realm: &mut Realm, profile: &Profile) -> Result<Sandbox> {
    profile.validate()?;

    let misses = Misses::default();
    let mut installed = Vec::new();

    realm.eval_unit(EVENT_TARGET, "wre:event-target")?;
    realm.eval_unit(SCAFFOLD, "wre:scaffold")?;

    native_noop(realm, "__wreAddListener", "addEventListener", Value::Null)?;
    native_noop(realm, "__wreRemoveListener", "removeEventListener", Value::Null)?;
    native_noop(realm, "__wreDispatchEvent", "dispatchEvent", json!(true))?;
    realm.eval_unit(EVENT_TARGET_METHODS, "wre:event-target-methods")?;

    for interface in &profile.interfaces {
        let parent = if interface.constructor == "Window" { "EventTarget" } else { "" };

        realm.eval_unit(
            &format!(
                "__wreInterface({}, {});",
                json!(interface.constructor),
                json!(parent)
            ),
            "wre:interface",
        )?;

        realm.brand_object(&format!("{}.prototype", interface.constructor), &interface.brand)?;

        realm.eval_unit(
            &format!(
                "__wreInstance({}, {});",
                json!(interface.constructor),
                json!(interface.instance)
            ),
            "wre:instance",
        )?;

        for (name, value) in &interface.properties {
            let host = format!("__wre${}${}", interface.brand, name);
            let answer = value.clone();

            realm.register_branded_host(
                &host,
                &interface.brand,
                Box::new(move |_args| Ok(answer.clone())),
            )?;

            realm.eval_unit(
                &format!(
                    "__wreDefine({}.prototype, {}, {host}); delete globalThis[{}];",
                    interface.constructor,
                    json!(name),
                    json!(host)
                ),
                "wre:accessor",
            )?;

            installed.push(format!("{}.{name}", interface.constructor));
        }
    }

    install_plugins(realm, profile)?;
    install_webgl(realm, profile, &misses)?;
    install_media(realm, profile, &misses)?;
    install_queries(realm, profile, &misses)?;

    realm.eval_unit(
        "delete globalThis.__wreDefine; delete globalThis.__wreInterface; \
         delete globalThis.__wreInstance; delete globalThis.__wreTag;",
        "wre:cleanup",
    )?;

    Ok(Sandbox { misses, installed })
}

fn install_plugins(realm: &mut Realm, profile: &Profile) -> Result<()> {
    if profile.plugins.is_empty() {
        return Ok(());
    }

    let entries = serde_json::to_string(&profile.plugins)
        .map_err(|error| Error::msg(format!("plugins did not serialise: {error}")))?;

    realm.eval_unit(
        &format!(
            r#"
(function () {{
  var described = {entries};

  var Plugin = __wreTag(function () {{ throw new TypeError("Illegal constructor"); }}, "Plugin");
  var PluginArray = __wreTag(
    function () {{ throw new TypeError("Illegal constructor"); }},
    "PluginArray"
  );

  var list = Object.create(PluginArray.prototype);

  described.forEach(function (entry, index) {{
    var plugin = Object.create(Plugin.prototype);
    Object.defineProperty(plugin, "name", {{ value: entry.name, enumerable: true }});
    Object.defineProperty(plugin, "filename", {{ value: entry.filename, enumerable: true }});
    Object.defineProperty(plugin, "description", {{ value: entry.description, enumerable: true }});
    Object.defineProperty(plugin, "length", {{ value: 0, enumerable: true }});

    Object.defineProperty(list, String(index), {{ value: plugin, enumerable: true }});
    Object.defineProperty(list, entry.name, {{ value: plugin, enumerable: false }});
  }});

  Object.defineProperty(list, "length", {{ value: described.length, enumerable: true }});
  PluginArray.prototype.item = function (index) {{ return this[index] || null; }};
  PluginArray.prototype.namedItem = function (name) {{ return this[name] || null; }};

  Object.defineProperty(Navigator.prototype, "plugins", {{
    get: function () {{ return list; }},
    enumerable: true,
    configurable: true
  }});
}})();
"#
        ),
        "wre:plugins",
    )
}

fn install_webgl(realm: &mut Realm, profile: &Profile, misses: &Misses) -> Result<()> {
    let parameters: BTreeMap<String, Value> = profile.webgl_parameters.clone();
    let watcher = misses.clone();

    realm.register_host(
        "__wreWebglParameter",
        Box::new(move |args| {
            let key = args
                .first()
                .map(render_key)
                .unwrap_or_else(|| "undefined".to_string());

            match parameters.get(&key) {
                Some(value) => Ok(value.clone()),
                None => {
                    watcher.record(&format!("webgl getParameter({key})"));
                    Ok(Value::Null)
                }
            }
        }),
    )?;

    let extensions = profile.webgl_extensions.clone();
    realm.register_host(
        "__wreWebglExtensions",
        Box::new(move |_args| Ok(json!(extensions))),
    )?;

    realm.eval_unit(
        r#"
(function () {
  var Ctx = __wreTag(
    function () { throw new TypeError("Illegal constructor"); },
    "WebGLRenderingContext"
  );

  Ctx.prototype.getParameter = __wreWebglParameter;
  Ctx.prototype.getSupportedExtensions = __wreWebglExtensions;
  Ctx.prototype.getExtension = function (name) {
    var supported = this.getSupportedExtensions() || [];
    if (supported.indexOf(name) < 0) return null;
    if (name === "WEBGL_debug_renderer_info") {
      return { UNMASKED_VENDOR_WEBGL: 37445, UNMASKED_RENDERER_WEBGL: 37446 };
    }
    return {};
  };

  delete globalThis.__wreWebglParameter;
  delete globalThis.__wreWebglExtensions;
})();
"#,
        "wre:webgl",
    )
}

fn install_media(realm: &mut Realm, profile: &Profile, misses: &Misses) -> Result<()> {
    let support = profile.media_support.clone();
    let watcher = misses.clone();

    realm.register_host(
        "__wreCanPlayType",
        Box::new(move |args| {
            let key = args
                .first()
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string();

            match support.get(&key) {
                Some(answer) => Ok(json!(answer)),
                None => {
                    watcher.record(&format!("canPlayType({key})"));
                    Ok(json!(""))
                }
            }
        }),
    )?;

    realm.eval_unit(
        r#"
(function () {
  var Media = __wreTag(
    function () { throw new TypeError("Illegal constructor"); },
    "HTMLMediaElement"
  );
  Media.prototype.canPlayType = __wreCanPlayType;
  delete globalThis.__wreCanPlayType;
})();
"#,
        "wre:media",
    )
}

fn state_accessor(
    realm: &mut Realm,
    slot: &str,
    brand: &str,
    field: &'static str,
) -> Result<()> {
    realm.register(
        HostSpec::new(slot)
            .called(&format!("get {field}"))
            .on_brand(brand)
            .with_state(),
        Box::new(move |args| {
            Ok(args
                .first()
                .and_then(|state| state.get(field))
                .cloned()
                .unwrap_or(Value::Null))
        }),
    )
}

fn native_noop(realm: &mut Realm, slot: &str, display: &str, answer: Value) -> Result<()> {
    realm.register(
        HostSpec::new(slot).called(display).taking(1),
        Box::new(move |_args| Ok(answer.clone())),
    )
}

fn install_queries(realm: &mut Realm, profile: &Profile, misses: &Misses) -> Result<()> {
    realm.eval_unit(
        r#"
__wreInterface("MediaQueryList", "EventTarget");
__wreInterface("PermissionStatus", "EventTarget");
__wreInterface("Permissions", "");
"#,
        "wre:query-interfaces",
    )?;

    realm.brand_object("MediaQueryList.prototype", "MediaQueryList")?;
    realm.brand_object("PermissionStatus.prototype", "PermissionStatus")?;
    realm.brand_object("Permissions.prototype", "Permissions")?;

    let queries = profile.media_queries.clone();
    let watcher = misses.clone();

    realm.register(
        HostSpec::new("__wreMatchMedia")
            .called("matchMedia")
            .taking(1)
            .building(Shape::new("MediaQueryList.prototype").branded("MediaQueryList")),
        Box::new(move |args| {
            let key = args
                .first()
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string();

            let matches = match queries.get(&key) {
                Some(answer) => *answer,
                None => {
                    watcher.record(&format!("matchMedia({key})"));
                    false
                }
            };

            Ok(json!({ "media": key, "matches": matches }))
        }),
    )?;

    state_accessor(realm, "__wreMqlMedia", "MediaQueryList", "media")?;
    state_accessor(realm, "__wreMqlMatches", "MediaQueryList", "matches")?;
    native_noop(realm, "__wreMqlAdd", "addListener", Value::Null)?;
    native_noop(realm, "__wreMqlRemove", "removeListener", Value::Null)?;

    let permissions = profile.permissions.clone();
    let watcher = misses.clone();

    realm.register(
        HostSpec::new("__wrePermissionQuery")
            .called("query")
            .taking(1)
            .on_brand("Permissions")
            .building(
                Shape::new("PermissionStatus.prototype")
                    .branded("PermissionStatus")
                    .in_a_promise(),
            ),
        Box::new(move |args| {
            let key = args
                .first()
                .and_then(|spec| spec.get("name"))
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string();

            match permissions.get(&key) {
                Some(state) => Ok(json!({ "name": key, "state": state })),
                None => {
                    watcher.record(&format!("permissions.query({key})"));
                    Ok(json!({ "name": key, "state": "prompt" }))
                }
            }
        }),
    )?;

    state_accessor(realm, "__wrePermissionName", "PermissionStatus", "name")?;
    state_accessor(realm, "__wrePermissionState", "PermissionStatus", "state")?;

    realm.register(
        HostSpec::new("__wreNavigatorPermissions")
            .called("get permissions")
            .on_brand("Navigator")
            .building(Shape::new("Permissions.prototype").shared()),
        Box::new(|_args| Ok(json!({}))),
    )?;

    realm.eval_unit(
        r#"
(function () {
  __wreDefine(MediaQueryList.prototype, "media", __wreMqlMedia);
  __wreDefine(MediaQueryList.prototype, "matches", __wreMqlMatches);
  MediaQueryList.prototype.addListener = __wreMqlAdd;
  MediaQueryList.prototype.removeListener = __wreMqlRemove;

  Permissions.prototype.query = __wrePermissionQuery;
  __wreDefine(PermissionStatus.prototype, "name", __wrePermissionName);
  __wreDefine(PermissionStatus.prototype, "state", __wrePermissionState);
  __wreDefine(Navigator.prototype, "permissions", __wreNavigatorPermissions);

  globalThis.matchMedia = __wreMatchMedia;

  [
    "__wreMatchMedia", "__wreMqlMedia", "__wreMqlMatches", "__wreMqlAdd", "__wreMqlRemove",
    "__wrePermissionQuery", "__wrePermissionName", "__wrePermissionState",
    "__wreNavigatorPermissions"
  ].forEach(function (slot) { delete globalThis[slot]; });
})();
"#,
        "wre:queries",
    )
}

fn render_key(value: &Value) -> String {
    match value {
        Value::Number(number) => number.to_string(),
        Value::String(text) => text.clone(),
        other => other.to_string(),
    }
}
