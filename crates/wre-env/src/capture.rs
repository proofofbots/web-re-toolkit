pub const CAPTURE: &str = r#"
(function (options) {
  var maxDepth = options.depth || 4;
  var maxProps = options.maxProps || 400;
  var maxString = options.maxString || 4096;

  var objects = [];
  var ids = new Map();
  var pending = [];

  function encode(value, depth) {
    var kind = typeof value;

    if (value === null) { return { k: "null" }; }
    if (kind === "undefined") { return { k: "undef" }; }
    if (kind === "boolean") { return { k: "bool", v: value }; }
    if (kind === "number") {
      if (Number.isNaN(value)) { return { k: "nan" }; }
      if (value === Infinity) { return { k: "inf", v: 1 }; }
      if (value === -Infinity) { return { k: "inf", v: -1 }; }
      return { k: "num", v: value };
    }
    if (kind === "string") {
      return { k: "str", v: value.length > maxString ? value.slice(0, maxString) : value };
    }
    if (kind === "bigint") { return { k: "bigint", v: String(value) }; }
    if (kind === "symbol") { return { k: "symbol", v: String(value) }; }

    if (depth >= maxDepth) { return { k: "deep" }; }

    return { k: "ref", id: idOf(value, depth) };
  }

  function className(value) {
    try {
      return Object.prototype.toString.call(value).slice(8, -1);
    } catch (error) {
      return "Unknown";
    }
  }

  function idOf(value, depth) {
    if (ids.has(value)) { return ids.get(value); }

    var id = objects.length;
    ids.set(value, id);

    var record = {
      id: id,
      cls: className(value),
      callable: typeof value === "function",
      props: {},
      getters: [],
      throwing: [],
      proto: null
    };

    if (record.callable) {
      try { record.fnName = value.name || ""; } catch (error) { record.fnName = ""; }
      try { record.fnLength = value.length || 0; } catch (error) { record.fnLength = 0; }
      try { record.native = /\{\s*\[native code\]\s*\}/.test(Function.prototype.toString.call(value)); }
      catch (error) { record.native = false; }
    }

    if (Array.isArray(value)) {
      record.array = true;
      record.length = value.length;
    }

    objects.push(record);
    pending.push({ value: value, record: record, depth: depth });
    return id;
  }

  function fill(entry) {
    var value = entry.value;
    var record = entry.record;
    var depth = entry.depth;

    var names;
    try { names = Object.getOwnPropertyNames(value); }
    catch (error) { names = []; }

    if (names.length > maxProps) { names = names.slice(0, maxProps); }

    for (var i = 0; i < names.length; i++) {
      var name = names[i];
      if (name === "constructor" && record.cls !== "Object") { continue; }

      var descriptor;
      try { descriptor = Object.getOwnPropertyDescriptor(value, name); }
      catch (error) { record.throwing.push(name); continue; }
      if (!descriptor) { continue; }

      if (descriptor.get) {
        record.getters.push(name);
        try {
          record.props[name] = encode(descriptor.get.call(value), depth + 1);
        } catch (error) {
          record.throwing.push(name);
          record.props[name] = { k: "undef" };
        }
        continue;
      }

      try {
        record.props[name] = encode(descriptor.value, depth + 1);
      } catch (error) {
        record.throwing.push(name);
      }
    }

    try {
      var proto = Object.getPrototypeOf(value);
      if (proto && depth + 1 < maxDepth) {
        record.proto = idOf(proto, depth + 1);
      }
    } catch (error) {}
  }

  var roots = {};
  var rootNames = options.roots || [
    "window", "navigator", "screen", "document", "location", "history",
    "performance", "crypto", "Intl", "console"
  ];

  for (var i = 0; i < rootNames.length; i++) {
    var name = rootNames[i];
    try {
      var value = (typeof globalThis[name] !== "undefined") ? globalThis[name] : undefined;
      if (value !== undefined && value !== null && typeof value === "object") {
        roots[name] = idOf(value, 0);
      }
    } catch (error) {}
  }

  var guard = 0;
  while (pending.length && guard < 200000) {
    guard += 1;
    fill(pending.shift());
  }

  var globals = [];
  try {
    globals = Object.getOwnPropertyNames(globalThis).slice(0, 2000);
  } catch (error) {}

  return {
    version: 1,
    capturedAt: Date.now(),
    href: (typeof location !== "undefined" && location.href) ? location.href : "",
    userAgent: (typeof navigator !== "undefined" && navigator.userAgent) ? navigator.userAgent : "",
    roots: roots,
    objects: objects,
    globals: globals,
    truncated: pending.length > 0
  };
})(__WRE_SNAPSHOT_OPTIONS__);
"#;

pub const MATERIALIZE: &str = r#"
(function (snapshot, options) {
  var built = new Array(snapshot.objects.length);
  var wired = new Array(snapshot.objects.length);
  var bridge = options.bridge || null;
  var recordCalls = options.recordCalls !== false;
  var missing = [];

  function shell(record) {
    if (record.callable) {
      var name = record.fnName || "anonymous";
      var made = function () {
        if (bridge) {
          var reply = bridge(record.path || name, Array.prototype.slice.call(arguments));
          if (reply !== undefined) { return reply; }
        }
        if (recordCalls && globalThis.__wre) {
          globalThis.__wre.push("calls", { fn: record.path || name, args: [] });
        }
        return undefined;
      };
      try {
        Object.defineProperty(made, "name", { value: name, configurable: true });
        Object.defineProperty(made, "length", { value: record.fnLength || 0, configurable: true });
      } catch (error) {}
      return made;
    }

    if (record.array) { return new Array(record.length || 0); }
    return {};
  }

  function decode(value) {
    switch (value.k) {
      case "null": return null;
      case "undef": return undefined;
      case "bool": return value.v;
      case "num": return value.v;
      case "str": return value.v;
      case "nan": return NaN;
      case "inf": return value.v > 0 ? Infinity : -Infinity;
      case "bigint": return BigInt(value.v);
      case "symbol": return Symbol(value.v);
      case "deep": return undefined;
      case "ref": return instantiate(value.id);
      default: return undefined;
    }
  }

  function instantiate(id) {
    if (built[id] !== undefined) { return built[id]; }
    var record = snapshot.objects[id];
    if (!record) { return undefined; }
    built[id] = shell(record);
    return built[id];
  }

  function wire(id) {
    if (wired[id]) { return; }
    wired[id] = true;

    var record = snapshot.objects[id];
    var target = instantiate(id);
    if (!record || target === undefined || target === null) { return; }

    var names = Object.keys(record.props);
    for (var i = 0; i < names.length; i++) {
      var name = names[i];
      var encoded = record.props[name];

      (function (name, encoded) {
        var cached;
        var resolved = false;

        try {
          Object.defineProperty(target, name, {
            configurable: true,
            enumerable: true,
            get: function () {
              if (!resolved) {
                cached = decode(encoded);
                if (encoded.k === "ref") { wire(encoded.id); }
                resolved = true;
              }
              return cached;
            },
            set: function (value) { cached = value; resolved = true; }
          });
        } catch (error) {
          missing.push(name);
        }
      })(name, encoded);
    }

    if (record.proto !== null && record.proto !== undefined) {
      try {
        var proto = instantiate(record.proto);
        wire(record.proto);
        if (proto && proto !== target) { Object.setPrototypeOf(target, proto); }
      } catch (error) {}
    }
  }

  var names = Object.keys(snapshot.roots);
  for (var i = 0; i < names.length; i++) {
    var id = snapshot.roots[names[i]];
    var value = instantiate(id);
    wire(id);
    try {
      globalThis[names[i]] = value;
    } catch (error) {
      missing.push(names[i]);
    }
  }

  if (snapshot.roots.window === undefined) {
    globalThis.window = globalThis;
  } else {
    try { globalThis.window = globalThis; } catch (error) {}
  }

  globalThis.self = globalThis;
  globalThis.top = globalThis;
  globalThis.parent = globalThis;

  return { roots: names, objects: snapshot.objects.length, missing: missing };
})(__WRE_SNAPSHOT__, __WRE_MATERIALIZE_OPTIONS__);
"#;
