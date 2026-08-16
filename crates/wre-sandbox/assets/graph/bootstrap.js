(function () {
  var snapshot = JSON.parse(globalThis.__SNAPSHOT);
  var bridge = globalThis.__BRIDGE;
  var isFrame = Boolean(globalThis.__FRAME);

  delete globalThis.__SNAPSHOT;
  delete globalThis.__BRIDGE;
  delete globalThis.__FRAME;

  var nativeApply = Reflect.apply;
  var nativeHasOwn = Object.prototype.hasOwnProperty;
  var nativeGet = Reflect.get;
  var nativeDescriptor = Reflect.getOwnPropertyDescriptor;
  var hasOwn = function (object, key) { return nativeApply(nativeHasOwn, object, [key]); };
  var indexedName = function (name) { return /^(0|[1-9][0-9]*)$/.test(name); };

  var sources = (bridge && bridge.sources) || new WeakMap();
  var built = new Map();
  var overrides = new WeakMap();
  var locked = [];
  var calls = Object.create(null);
  var misses = Object.create(null);

  var nativeSource = function (name) {
    return "function " + name + "() { [native code] }";
  };

  var trail = [];

  var virtualMs = 0;
  var costs = null;
  var resolutionMs = 0;
  var timeScale = 1;
  var defaultGet = 0.00005;
  var defaultCall = 0.0002;

  var costOf = function (label) {
    if (!costs) return 0;

    var known = costs[label];
    if (known !== undefined) return known;

    var cut = label.indexOf(":");

    if (cut !== -1) {
      var base = costs[label.slice(0, cut)];

      if (base !== undefined) {
        costs[label] = base;
        return base;
      }
    }

    var fallback = label.indexOf("get ") === 0 ? defaultGet : defaultCall;
    costs[label] = fallback;
    return fallback;
  };

  var useTiming = function (table) {
    costs = Object.create(null);
    resolutionMs = table.resolutionMs || 0;
    if (typeof table.scale === "number" && table.scale >= 0) timeScale = table.scale;

    var names = Object.keys(table.calls || {});

    for (var index = 0; index < names.length; index += 1) {
      var value = table.calls[names[index]];
      if (typeof value === "number") costs[names[index]] = value / 1e6;
    }
  };

  var clock = function () {
    var raw = bridge.now() * timeScale + virtualMs;
    if (!resolutionMs) return raw;
    return Math.floor(raw / resolutionMs) * resolutionMs;
  };

  var events = [];
  var recording = false;
  var hooks = { stack: null };

  var nativeIsArray = Array.isArray;
  var objToString = Object.prototype.toString;

  var preview = function (value, depth) {
    try {
      if (value === null || value === undefined) return String(value);

      var kind = typeof value;

      if (kind === "string") return value.length > 160 ? value.slice(0, 160) + "..." : value;
      if (kind === "number" || kind === "boolean" || kind === "bigint" || kind === "symbol") return String(value);
      if (kind === "function") return "function " + (value.name || "");

      if (nativeIsArray(value)) {
        if (depth > 1) return "[array " + value.length + "]";

        var parts = [];
        for (var index = 0; index < value.length && index < 12; index += 1) parts.push(preview(value[index], depth + 1));
        return "[" + parts.join(", ") + (value.length > 12 ? ", ..." : "") + "]";
      }

      return nativeApply(objToString, value, []);
    } catch (error) {
      return "?";
    }
  };

  var note = function (at, args, value) {
    if (at < 0 || at >= events.length) return;

    var entry = events[at];

    if (args) {
      var parts = [];
      for (var index = 0; index < args.length && index < 6; index += 1) parts.push(preview(args[index], 1));
      entry[1] = parts.join(", ");
    }

    entry[2] = preview(value, 0);
  };

  var count = function (table, key) {
    table[key] = (table[key] || 0) + 1;
    if (costs) virtualMs += costOf(key);
    trail.push(key);
    if (trail.length > 400) trail.shift();
    if (!recording) return -1;
    events.push([key, "", "", hooks.stack ? hooks.stack() : null]);
    return events.length - 1;
  };

  var behaviour = Object.create(null);

  var decode = function (entry, label) {
    if (!entry) return undefined;

    switch (entry.k) {
      case "null":
        return null;
      case "boolean":
      case "number":
      case "string":
        return entry.v;
      case "bigint":
        return BigInt(entry.v);
      case "symbol":
        return Symbol(entry.v);
      case "ref":
        return materialize(entry.id, label);
      default:
        return undefined;
    }
  };

  var PROTO_KEY = "\u0000proto";
  var METHOD_SHELL = Object.getOwnPropertyDescriptor(Map.prototype, "get").value;

  var shells = { toString: Object.prototype.toString, valueOf: Object.prototype.valueOf };

  var homes = new WeakMap();
  var hosts = new WeakSet();

  var getPrototypeOf = Object.getPrototypeOf;

  var isInterfacePrototype = function (home) {
    try {
      var ctor = home.constructor;
      return typeof ctor === "function" && ctor.prototype === home;
    } catch (error) {
      return false;
    }
  };

  var ownsReceiver = function (home, self) {
    if (self === home) return !isInterfacePrototype(home);
    if (self === null || self === undefined) return false;
    if (typeof self !== "object" && typeof self !== "function") return false;

    var current = self;

    for (var steps = 0; steps < 64; steps += 1) {
      try {
        current = getPrototypeOf(current);
      } catch (error) {
        return false;
      }

      if (!current) return false;
      if (current === home) return true;
    }

    return false;
  };

  var FRAME_ROOM = 24;

  var trimFrames = function (error, keep) {
    try {
      var lines = String(error.stack || "").split("\n");
      var kept = [];

      for (var index = 0; index < lines.length; index += 1) {
        var line = lines[index];
        var harness = index > 0 && line.indexOf("eval at") === -1 && /<anonymous>:\d+:\d+\)?\s*$/.test(line);
        if (!harness) kept.push(line);
      }

      if (typeof keep === "number" && isFinite(keep) && kept.length > keep + 1) kept.length = keep + 1;

      error.stack = kept.join("\n");
    } catch (ignored) {}

    return error;
  };

  var withStackRoom = function (run) {
    var limit = Error.stackTraceLimit;
    var raised = typeof limit === "number" && isFinite(limit);

    if (raised) Error.stackTraceLimit = limit + FRAME_ROOM;

    try {
      return run(limit);
    } finally {
      if (raised) Error.stackTraceLimit = limit;
    }
  };

  var hideFrames = function (error) {
    var limit = Error.stackTraceLimit;

    if (typeof Error.captureStackTrace === "function" && typeof limit === "number" && isFinite(limit)) {
      try {
        Error.stackTraceLimit = limit + FRAME_ROOM;
        Error.captureStackTrace(error, hideFrames);
      } catch (ignored) {
      } finally {
        Error.stackTraceLimit = limit;
      }
    }

    return trimFrames(error, limit);
  };

  var illegalLabels = new WeakMap();

  var illegal = function (label) {
    misses["illegal:" + String(label || "")] = (misses["illegal:" + String(label || "")] || 0) + 1;

    var parts = String(label || "").split(".");
    var thrown =
      parts[0] === "chrome" && parts.length >= 3
        ? hideFrames(new TypeError("Error in invocation of " + parts.slice(-2).join(".") + "(): "))
        : hideFrames(new TypeError("Illegal invocation"));

    illegalLabels.set(thrown, String(label || ""));
    throw thrown;
  };

  var asMethod = function (implementation, name, length) {
    var own = {
      name: name === undefined ? "" : String(name),
      length: typeof length === "number" ? length : implementation.length,
    };

    return new Proxy(shells[own.name] || METHOD_SHELL, {
      apply: function (target, self, args) {
        return implementation.apply(self, args);
      },
      get: function (target, key, receiver) {
        if (hasOwn(own, key)) return own[key];
        return nativeGet(target, key, receiver);
      },
      set: function (target, key, value) {
        own[key] = value;
        return true;
      },
      getOwnPropertyDescriptor: function (target, key) {
        if (hasOwn(own, key)) {
          return { value: own[key], writable: false, enumerable: false, configurable: true };
        }

        return nativeDescriptor(target, key);
      },
      defineProperty: function (target, key, descriptor) {
        if ("value" in descriptor) own[key] = descriptor.value;
        return true;
      },
      getPrototypeOf: function (target) {
        return hasOwn(own, PROTO_KEY) ? own[PROTO_KEY] : getPrototypeOf(target);
      },
      setPrototypeOf: function (target, value) {
        own[PROTO_KEY] = value;
        return true;
      },
    });
  };

  var validName = /^[A-Za-z_$][A-Za-z0-9_$]*$/;

  var nameFactories = Object.create(null);

  var withName = function (name, implementation) {
    if (!validName.test(name)) return implementation;

    if (!nameFactories[name]) {
      try {
        nameFactories[name] = new Function("inner", "apply", "return function " + name + "() { return apply(inner, this, arguments); }");
      } catch (error) {
        return implementation;
      }
    }

    return nameFactories[name](implementation, nativeApply);
  };

  var makeFunction = function (record, label) {
    var stub;

    var body = function () {
      var home = homes.get(stub);
      if (home !== undefined && typeof home !== "function" && !ownsReceiver(home, this)) illegal(label);

      var at = count(calls, label);
      var implementation = behaviour[label];

      if (!implementation) {
        if (at >= 0) note(at, arguments, undefined);
        return undefined;
      }

      var outcome = implementation.apply(this, arguments);
      if (at >= 0) note(at, arguments, outcome);
      return outcome;
    };

    var constructible = Boolean(record.props && record.props.prototype);
    stub = constructible ? withName(record.name || "", body) : asMethod(body, record.name || "", record.length || 0);

    try {
      Object.defineProperty(stub, "name", { value: record.name || "", configurable: true });
      Object.defineProperty(stub, "length", { value: record.length || 0, configurable: true });
    } catch (error) {}

    sources.set(stub, record.source || nativeSource(record.name || ""));
    return stub;
  };

  var materialize = function (id, label, existing) {
    if (built.has(id) && !existing) return built.get(id);

    var record = snapshot.objects[id];
    if (!record) return undefined;

    var target = existing || (record.type === "function" ? makeFunction(record, label) : {});
    built.set(id, target);
    if (target !== globalThis) hosts.add(target);

    var keys = Object.keys(record.props || {});

    for (var index = 0; index < keys.length; index += 1) {
      var key = keys[index];

      if (key.indexOf("@@") === 0) {
        var wellKnown = key.slice(2);

        if (wellKnown.indexOf("Symbol.") === 0) {
          var symbolName = wellKnown.slice(7);

          if (Symbol[symbolName]) {
            try {
              Object.defineProperty(target, Symbol[symbolName], {
                value: decode(record.props[key].value, label + "." + key),
                writable: Boolean(record.props[key].w),
                enumerable: Boolean(record.props[key].e),
                configurable: true,
              });
            } catch (error) {}
          }
        }

        continue;
      }

      var prop = record.props[key];
      var member = label ? label + "." + key : key;


      if (record.type === "function" && (key === "name" || key === "length")) continue;
      if (target === globalThis && indexedName(key)) continue;
      if (target === globalThis && builtin[key] && globalThis[key] !== undefined) continue;
      if (target === globalThis && isFrame && key === "KPSDK") continue;

      if (target === globalThis && prop.value && prop.value.k === "page") {
        try {
          if (!(key in globalThis)) {
            Object.defineProperty(globalThis, key, {
              value: prop.value.t === "function" ? makeFunction({ name: key, length: 0 }, "window." + key) : {},
              writable: Boolean(prop.w),
              enumerable: Boolean(prop.e),
              configurable: true,
            });
          }
        } catch (error) {}

        continue;
      }

      try {
        if (prop.accessor) {
          (function (prop, key, member, home) {
            var cached = decode(prop.read, member);

            Object.defineProperty(target, key, {
              get: function () {
                if (home !== globalThis && !ownsReceiver(home, this)) illegal(member);

                var at = count(calls, "get " + member);
                var perInstance = overrides.get(this);
                var outcome;

                if (perInstance && key in perInstance) {
                  outcome = perInstance[key];
                } else {
                  var implementation = behaviour["get " + member];
                  outcome = implementation ? implementation.call(this) : cached;
                }

                if (at >= 0) note(at, null, outcome);
                return outcome;
              },
              set: prop.set
                ? function (value) {
                    var implementation = behaviour["set " + member];
                    if (implementation) implementation.call(this, value);
                  }
                : undefined,
              enumerable: Boolean(prop.e),
              configurable: true,
            });

            if (!prop.c) locked.push([target, key]);

            var descriptor = Object.getOwnPropertyDescriptor(target, key);
            if (descriptor && descriptor.get) sources.set(descriptor.get, nativeSource("get " + key));
            if (descriptor && descriptor.set) sources.set(descriptor.set, nativeSource("set " + key));
          })(prop, key, member, target);
        } else if (key === "prototype" && typeof target === "function") {
          var interfacePrototype = decode(prop.value, member);
          if (interfacePrototype !== undefined) target.prototype = interfacePrototype;
        } else {
          var value = decode(prop.value, member);
          if (typeof value === "function" && !homes.has(value)) homes.set(value, target);

          Object.defineProperty(target, key, {
            value: value,
            writable: Boolean(prop.w),
            enumerable: Boolean(prop.e),
            configurable: true,
          });

          if (!prop.c) locked.push([target, key]);
        }
      } catch (error) {}
    }

    try {
      if (record.proto) {
        var proto = record.proto.k === "null" ? null : decode(record.proto, label + ".[[proto]]");
        if (proto !== undefined) Object.setPrototypeOf(target, proto);
        try {
          if (id === 0 || id === 194 || id === 3067 || id === 3665) {
            globalThis.__PROTOOK = globalThis.__PROTOOK || [];
            globalThis.__PROTOOK.push([id, label, proto === undefined ? "undefined" : String(Object.prototype.toString.call(proto))]);
          }
        } catch (ignored) {}
      }
    } catch (error) {}

    return target;
  };

  var builtin = {};
  var builtinNames = [
    "Array", "ArrayBuffer", "BigInt", "Boolean", "DataView", "Date", "Error", "EvalError", "Float32Array",
    "Float64Array", "Function", "Int16Array", "Int32Array", "Int8Array", "Intl", "JSON", "Map", "Math",
    "Number", "Object", "Promise", "Proxy", "RangeError", "ReferenceError", "Reflect", "RegExp", "Set",
    "String", "Symbol", "SyntaxError", "TypeError", "URIError", "Uint16Array", "Uint32Array", "Uint8Array",
    "Uint8ClampedArray", "WeakMap", "WeakSet", "decodeURI", "decodeURIComponent", "encodeURI",
    "encodeURIComponent", "escape", "eval", "isFinite", "isNaN", "parseFloat", "parseInt", "unescape",
    "WebAssembly", "SharedArrayBuffer", "globalThis", "undefined", "NaN", "Infinity", "console",
    "TextEncoder", "TextDecoder", "atob", "btoa", "structuredClone", "queueMicrotask",
  ];

  for (var b = 0; b < builtinNames.length; b += 1) builtin[builtinNames[b]] = true;

  var seed = function (id, real, depth) {
    if (id === null || id === undefined || built.has(id)) return;

    var record = snapshot.objects[id];
    if (!record) return;

    if (record.tag) {
      var brand;

      try {
        brand = Object.prototype.toString.call(real);
      } catch (error) {
        return;
      }

      if (brand !== record.tag) return;
    }

    built.set(id, real);
    if (depth <= 0) return;

    var keys = Object.keys(record.props || {});

    for (var index = 0; index < keys.length; index += 1) {
      var key = keys[index];
      var prop = record.props[key];
      if (!prop || !prop.value || prop.value.k !== "ref") continue;

      var actual;

      try {
        actual = real[key];
      } catch (error) {
        continue;
      }

      if (actual === null || (typeof actual !== "object" && typeof actual !== "function")) continue;
      seed(prop.value.id, actual, depth - 1);
    }

    if (record.proto && record.proto.k === "ref") {
      try {
        var proto = Object.getPrototypeOf(real);
        if (proto) seed(record.proto.id, proto, depth - 1);
      } catch (error) {}
    }
  };

  var windowRecord = snapshot.roots.window && snapshot.roots.window.k === "ref" ? snapshot.objects[snapshot.roots.window.id] : null;

  if (windowRecord) {
    var windowKeys = Object.keys(windowRecord.props || {});

    for (var w2 = 0; w2 < windowKeys.length; w2 += 1) {
      var builtinName = windowKeys[w2];
      if (!builtin[builtinName]) continue;

      var prop = windowRecord.props[builtinName];
      if (!prop || !prop.value || prop.value.k !== "ref") continue;

      var real = globalThis[builtinName];
      if (real === undefined || real === null) continue;

      seed(prop.value.id, real, 4);
    }
  }

  if (snapshot.roots.window && snapshot.roots.window.k === "ref") {
    materialize(snapshot.roots.window.id, "window", globalThis);
  }

  var linkRecordedChain = function () {
    if (!windowRecord || !windowRecord.proto || windowRecord.proto.k !== "ref") return;

    var target = Object.getPrototypeOf(globalThis);
    var id = windowRecord.proto.id;

    for (var step = 0; step < 8 && target && id !== null && id !== undefined; step += 1) {
      var record = snapshot.objects[id];
      if (!record) return;

      materialize(id, "window.[[proto]]", target);

      if (!record.proto || record.proto.k !== "ref") return;

      var next = materialize(record.proto.id, "window.[[proto]].[[proto]]");
      if (!next || next === target) return;

      try {
        Object.setPrototypeOf(target, next);
      } catch (error) {
        return;
      }

      target = next;
      id = record.proto.id;
    }
  };

  linkRecordedChain();

  var windowOrder = windowRecord
    ? Object.keys(windowRecord.props || {}).filter(function (name) {
        if (name.indexOf("@@") === 0) return false;
        if (indexedName(name)) return false;
        return !(isFrame && name === "KPSDK");
      })
    : [];

  var orderWindow = function () {
    if (!windowRecord) return;

    var root = globalThis;
    var defineProperty = Object.defineProperty;
    var describeOwn = Object.getOwnPropertyDescriptor;
    var ownNames = Object.getOwnPropertyNames;
    var order = Object.keys(windowRecord.props || {});
    var present = Object.create(null);

    for (var o = 0; o < order.length; o += 1) present[order[o]] = true;

    for (var index = 0; index < order.length; index += 1) {
      var key = order[index];
      if (key.indexOf("@@") === 0) continue;

      var descriptor;

      try {
        descriptor = describeOwn(root, key);
      } catch (error) {
        continue;
      }

      if (!descriptor || !descriptor.configurable) continue;

      try {
        delete root[key];
        defineProperty(root, key, descriptor);
      } catch (error) {}
    }

    var own = ownNames(root);

    for (var e = 0; e < own.length; e += 1) {
      var extra = own[e];
      if (present[extra] || extra === "__ENV" || extra === "__PUMP") continue;

      try {
        var check = describeOwn(root, extra);
        if (check && check.configurable) delete root[extra];
      } catch (error) {}
    }
  };

  var rootNames = Object.keys(snapshot.roots);

  for (var r = 0; r < rootNames.length; r += 1) {
    var name = rootNames[r];
    if (builtin[name] || name === "KPSDK") continue;

    try {
      var value = decode(snapshot.roots[name], name);
      if (value !== undefined) globalThis[name] = value;
    } catch (error) {}
  }

  var nativeToString = Function.prototype.toString;

  var patched = asMethod(function () {
    var recorded = sources.get(this);
    if (recorded) return recorded;

    var self = this;

    return withStackRoom(function (limit) {
      try {
        return nativeToString.call(self);
      } catch (error) {
        throw trimFrames(error, limit);
      }
    });
  }, "toString", 0);

  sources.set(patched, nativeSource("toString"));

  try {
    Object.defineProperty(Function.prototype, "toString", {
      value: patched,
      writable: true,
      enumerable: false,
      configurable: true,
    });
  } catch (error) {}

  var applyValues = function () {
    var names = Object.keys(snapshot.values || {});

    for (var index = 0; index < names.length; index += 1) {
      var name = names[index];
      var instance = name === "window" ? globalThis : resolve(name);
      if (!instance || (typeof instance !== "object" && typeof instance !== "function")) continue;

      var recorded = snapshot.values[name];
      var keys = Object.keys(recorded);
      var decoded = overrides.get(instance) || {};

      for (var k = 0; k < keys.length; k += 1) {
        try {
          decoded[keys[k]] = decode(recorded[keys[k]], name + "." + keys[k]);
        } catch (error) {}
      }

      overrides.set(instance, decoded);
    }
  };

  var resolve = function (path) {
    var parts = path.split(".");
    var current = globalThis;

    for (var index = 0; index < parts.length; index += 1) {
      if (current === null || current === undefined) return undefined;
      current = current[parts[index]];
    }

    return current;
  };

  applyValues();

  var seal = function () {
    for (var index = 0; index < locked.length; index += 1) {
      var target = locked[index][0];
      var key = locked[index][1];

      try {
        var descriptor = Object.getOwnPropertyDescriptor(target, key);
        if (!descriptor || !descriptor.configurable) continue;
        descriptor.configurable = false;
        Object.defineProperty(target, key, descriptor);
      } catch (error) {}
    }

    locked.length = 0;
  };

  globalThis.__ENV = {
    orderWindow: orderWindow,
    windowOrder: windowOrder,
    asMethod: asMethod,
    hideFrames: hideFrames,
    labelOf: function (error) {
      try {
        return illegalLabels.get(error) || null;
      } catch (failure) {
        return null;
      }
    },
    isHost: function (value) {
      return (value !== null && (typeof value === "object" || typeof value === "function") && hosts.has(value)) === true;
    },
    brandOf: function (value) {
      try {
        if (value.constructor && value.constructor.name) return value.constructor.name;
      } catch (error) {}

      return String(Object.prototype.toString.call(value)).slice(8, -1);
    },
    guard: function (home, self, label) {
      if (home !== globalThis && !ownsReceiver(home, self)) illegal(label);
    },
    rawStringify: JSON.stringify,
    count: function (label) { return count(calls, label); },
    note: note,
    events: events,
    recordEvents: function () { recording = true; },
    useTiming: useTiming,
    spend: function (ms) {
      if (typeof ms === "number" && isFinite(ms) && ms > 0) virtualMs += ms;
    },
    hooks: hooks,
    clock: clock,
    trail: trail,
    seal: seal,
    overrides: overrides,
    snapshot: snapshot,
    sources: sources,
    behaviour: behaviour,
    calls: calls,
    misses: misses,
    materialize: materialize,
    decode: decode,
    bridge: bridge,
    nativeSource: nativeSource,
  };
})();
