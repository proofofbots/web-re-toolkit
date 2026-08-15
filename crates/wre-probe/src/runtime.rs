pub const RUNTIME: &str = r#"
(function (config) {
  var root = typeof window !== "undefined" ? window : globalThis;
  if (root[config.name]) { return; }

  var started = Date.now();
  var reads = new Map();
  var calls = new Map();
  var events = [];
  var network = [];
  var notes = [];

  var nativeToString = Function.prototype.toString;
  var originals = new WeakMap();
  var patched = new WeakSet();

  function now() { return Date.now() - started; }

  function site() {
    if (!config.callSites) { return ""; }
    var raw = "";
    try { raw = new Error().stack || ""; } catch (error) { return ""; }
    var lines = raw.split("\n").slice(2, 8);
    for (var i = 0; i < lines.length; i++) {
      var line = lines[i];
      if (line.indexOf(config.name) < 0 && line.indexOf("<anonymous>:") < 0) {
        return line.trim().slice(0, 200);
      }
    }
    return (lines[0] || "").trim().slice(0, 200);
  }

  function describe(value) {
    try {
      if (value === null) { return "null"; }
      if (value === undefined) { return "undefined"; }
      var kind = typeof value;
      if (kind === "string") {
        return value.length > config.maxSampleLength
          ? value.slice(0, config.maxSampleLength) + "…"
          : value;
      }
      if (kind === "function") { return "[function " + (value.name || "anonymous") + "]"; }
      if (kind === "object") {
        if (Array.isArray(value)) { return "[array " + value.length + "]"; }
        var tag = Object.prototype.toString.call(value).slice(8, -1);
        return "[object " + tag + "]";
      }
      return String(value);
    } catch (error) {
      return "[unreadable]";
    }
  }

  function bump(store, key, sample) {
    var entry = store.get(key);
    if (!entry) {
      entry = { key: key, count: 0, first: now(), last: now(), samples: [], sites: [] };
      store.set(key, entry);
    }
    entry.count += 1;
    entry.last = now();
    if (entry.samples.length < config.maxSamples) {
      entry.samples.push(describe(sample));
    }
    if (entry.sites.length < config.maxSites) {
      var where = site();
      if (where && entry.sites.indexOf(where) < 0) { entry.sites.push(where); }
    }
    return entry;
  }

  function disguise(replacement, original) {
    if (!config.stealth) { return replacement; }
    patched.add(replacement);
    originals.set(replacement, original);
    try {
      Object.defineProperty(replacement, "name", { value: original.name, configurable: true });
      Object.defineProperty(replacement, "length", { value: original.length, configurable: true });
    } catch (error) {}
    return replacement;
  }

  if (config.stealth) {
    Function.prototype.toString = disguise(function toString() {
      var original = originals.get(this);
      if (original) { return nativeToString.call(original); }
      return nativeToString.call(this);
    }, nativeToString);
  }

  function resolve(path) {
    var parts = String(path).split(".");
    var holder = root;
    for (var i = 0; i < parts.length; i++) {
      if (holder === undefined || holder === null) { return null; }
      holder = holder[parts[i]];
    }
    return holder === undefined ? null : holder;
  }

  function trapProperty(holderPath, property, label) {
    var holder = resolve(holderPath);
    if (!holder) { notes.push("missing holder " + holderPath); return false; }

    var descriptor = Object.getOwnPropertyDescriptor(holder, property);
    if (!descriptor) { notes.push("missing property " + holderPath + "." + property); return false; }
    if (!descriptor.configurable) { notes.push("locked property " + holderPath + "." + property); return false; }

    var tag = label || (holderPath + "." + property);

    if (descriptor.get) {
      var realGet = descriptor.get;
      var getter = disguise(function () {
        var value = realGet.call(this);
        bump(reads, tag, value);
        return value;
      }, realGet);

      try {
        Object.defineProperty(holder, property, {
          get: getter,
          set: descriptor.set,
          configurable: true,
          enumerable: descriptor.enumerable
        });
        return true;
      } catch (error) {
        notes.push("could not redefine " + tag);
        return false;
      }
    }

    var stored = descriptor.value;
    try {
      Object.defineProperty(holder, property, {
        get: disguise(function () { bump(reads, tag, stored); return stored; }, function () {}),
        set: disguise(function (value) { stored = value; }, function () {}),
        configurable: true,
        enumerable: descriptor.enumerable
      });
      return true;
    } catch (error) {
      notes.push("could not redefine " + tag);
      return false;
    }
  }

  function trapMethod(holderPath, method, label) {
    var holder = resolve(holderPath);
    if (!holder) { notes.push("missing holder " + holderPath); return false; }

    var original = holder[method];
    if (typeof original !== "function") {
      notes.push("missing method " + holderPath + "." + method);
      return false;
    }

    var tag = label || (holderPath + "." + method);

    var wrapper = disguise(function () {
      var parts = [];
      for (var i = 0; i < arguments.length && i < config.maxArguments; i++) {
        parts.push(describe(arguments[i]));
      }
      var entry = bump(calls, tag, parts.join(", "));
      try {
        var value = original.apply(this, arguments);
        if (entry.results === undefined) { entry.results = []; }
        if (entry.results.length < config.maxSamples) { entry.results.push(describe(value)); }
        return value;
      } catch (error) {
        entry.threw = String((error && error.message) || error);
        throw error;
      }
    }, original);

    try {
      holder[method] = wrapper;
      return true;
    } catch (error) {
      notes.push("could not replace " + tag);
      return false;
    }
  }

  function recordRequest(record) {
    if (network.length < config.maxNetwork) { network.push(record); }
  }

  function trapNetwork() {
    if (typeof root.fetch === "function") {
      var realFetch = root.fetch;
      root.fetch = disguise(function (input, init) {
        var url = typeof input === "string" ? input : (input && input.url) || "";
        var method = (init && init.method) || (input && input.method) || "GET";
        var body = init && init.body ? describe(init.body) : null;
        recordRequest({ via: "fetch", url: String(url), method: method, body: body, at: now() });
        return realFetch.apply(this, arguments);
      }, realFetch);
    }

    if (typeof root.XMLHttpRequest === "function") {
      var proto = root.XMLHttpRequest.prototype;
      var realOpen = proto.open;
      var realSend = proto.send;
      var realHeader = proto.setRequestHeader;

      proto.open = disguise(function (method, url) {
        this.__wreMethod = method;
        this.__wreUrl = String(url);
        this.__wreHeaders = {};
        return realOpen.apply(this, arguments);
      }, realOpen);

      proto.setRequestHeader = disguise(function (name, value) {
        if (this.__wreHeaders) { this.__wreHeaders[String(name)] = String(value); }
        return realHeader.apply(this, arguments);
      }, realHeader);

      proto.send = disguise(function (body) {
        recordRequest({
          via: "xhr",
          url: this.__wreUrl || "",
          method: this.__wreMethod || "GET",
          headers: this.__wreHeaders || {},
          body: body === undefined || body === null ? null : describe(body),
          at: now()
        });
        return realSend.apply(this, arguments);
      }, realSend);
    }

    if (root.navigator && typeof root.navigator.sendBeacon === "function") {
      var realBeacon = root.navigator.sendBeacon;
      root.navigator.sendBeacon = disguise(function (url, data) {
        recordRequest({ via: "beacon", url: String(url), method: "POST", body: describe(data), at: now() });
        return realBeacon.apply(this, arguments);
      }, realBeacon);
    }
  }

  function trapWorkers() {
    if (typeof root.Blob === "function") {
      var RealBlob = root.Blob;
      var BlobShim = function (parts, options) {
        var made = new RealBlob(parts, options);
        try {
          if (parts && parts.length && typeof parts[0] === "string" && parts[0].length < config.maxBlobLength) {
            made.__wreSource = parts[0];
          }
        } catch (error) {}
        return made;
      };
      BlobShim.prototype = RealBlob.prototype;
      root.Blob = disguise(BlobShim, RealBlob);
    }

    if (typeof root.Worker === "function") {
      var RealWorker = root.Worker;
      var WorkerShim = function (url, options) {
        bump(calls, "Worker", String(url));
        return new RealWorker(url, options);
      };
      WorkerShim.prototype = RealWorker.prototype;
      root.Worker = disguise(WorkerShim, RealWorker);
    }
  }

  function trapEvents(names) {
    if (!names || !names.length) { return; }
    var realAdd = EventTarget.prototype.addEventListener;
    EventTarget.prototype.addEventListener = disguise(function (type) {
      if (names.indexOf(String(type)) >= 0 && events.length < config.maxEvents) {
        events.push({ type: String(type), at: now(), site: site() });
      }
      return realAdd.apply(this, arguments);
    }, realAdd);
  }

  var installed = { properties: 0, methods: 0, failed: 0 };

  (config.properties || []).forEach(function (entry) {
    if (trapProperty(entry.holder, entry.property, entry.label)) { installed.properties += 1; }
    else { installed.failed += 1; }
  });

  (config.methods || []).forEach(function (entry) {
    if (trapMethod(entry.holder, entry.method, entry.label)) { installed.methods += 1; }
    else { installed.failed += 1; }
  });

  if (config.network) { trapNetwork(); }
  if (config.workers) { trapWorkers(); }
  trapEvents(config.events || []);

  function drain(store) {
    var out = [];
    store.forEach(function (entry) { out.push(entry); });
    out.sort(function (left, right) { return right.count - left.count; });
    return out;
  }

  Object.defineProperty(root, config.name, {
    value: {
      version: config.version,
      installed: installed,
      notes: notes,
      dump: function () {
        return {
          startedAt: started,
          elapsed: now(),
          installed: installed,
          reads: drain(reads),
          calls: drain(calls),
          events: events,
          network: network,
          notes: notes
        };
      },
      reset: function () {
        reads = new Map();
        calls = new Map();
        events = [];
        network = [];
      }
    },
    enumerable: false,
    configurable: true,
    writable: false
  });
})(__WRE_CONFIG__);
"#;
