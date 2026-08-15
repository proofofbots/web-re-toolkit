pub const RUNTIME: &str = r#"
(function () {
  var records = { console: [], access: [], errors: [], calls: [] };
  Object.defineProperty(globalThis, "__wre", {
    value: {
      records: records,
      limit: 4096,
      push: function (bucket, entry) {
        var list = records[bucket];
        if (!list) { list = records[bucket] = []; }
        if (list.length < this.limit) { list.push(entry); }
      },
      drain: function () {
        var out = records;
        records = { console: [], access: [], errors: [], calls: [] };
        return out;
      },
      describe: function (value) {
        try {
          if (value === null) return "null";
          var kind = typeof value;
          if (kind === "string") return value.length > 200 ? value.slice(0, 200) + "…" : value;
          if (kind === "function") return "[function " + (value.name || "anonymous") + "]";
          if (kind === "object") {
            if (Array.isArray(value)) return "[array " + value.length + "]";
            return "[object " + Object.prototype.toString.call(value).slice(8, -1) + "]";
          }
          return String(value);
        } catch (error) {
          return "[unreadable]";
        }
      }
    },
    enumerable: false,
    configurable: false,
    writable: false
  });

  var levels = ["log", "info", "warn", "error", "debug", "trace"];
  var console = {};
  levels.forEach(function (level) {
    console[level] = function () {
      var parts = [];
      for (var i = 0; i < arguments.length; i++) parts.push(__wre.describe(arguments[i]));
      __wre.push("console", { level: level, text: parts.join(" ") });
    };
  });
  console.dir = console.log;
  console.table = console.log;
  console.group = console.log;
  console.groupEnd = function () {};
  console.time = function () {};
  console.timeEnd = function () {};
  console.assert = function () {};
  globalThis.console = console;
})();
"#;

pub fn clock(epoch_ms: f64) -> String {
    format!(
        r#"
(function () {{
  var fixed = {epoch_ms};
  var RealDate = Date;
  var step = 0;

  function FakeDate(value) {{
    if (!(this instanceof FakeDate)) return new RealDate(fixed).toString();
    if (arguments.length === 0) return new RealDate(fixed);
    if (arguments.length === 1) return new RealDate(value);
    return new RealDate(
      arguments[0], arguments[1], arguments.length > 2 ? arguments[2] : 1,
      arguments.length > 3 ? arguments[3] : 0, arguments.length > 4 ? arguments[4] : 0,
      arguments.length > 5 ? arguments[5] : 0, arguments.length > 6 ? arguments[6] : 0
    );
  }}

  FakeDate.now = function () {{ return fixed; }};
  FakeDate.parse = RealDate.parse;
  FakeDate.UTC = RealDate.UTC;
  FakeDate.prototype = RealDate.prototype;
  Object.defineProperty(FakeDate, "name", {{ value: "Date" }});
  globalThis.Date = FakeDate;

  var origin = fixed;
  globalThis.performance = globalThis.performance || {{}};
  globalThis.performance.now = function () {{ step += 0.1; return step; }};
  globalThis.performance.timeOrigin = origin;
  globalThis.performance.getEntriesByType = function () {{ return []; }};
  globalThis.performance.getEntriesByName = function () {{ return []; }};
  globalThis.performance.mark = function () {{}};
  globalThis.performance.measure = function () {{}};
}})();
"#
    )
}

pub fn random(seed: u64) -> String {
    format!(
        r#"
(function () {{
  var state = {seed} >>> 0;
  if (state === 0) state = 0x1a2b3c4d;
  Math.random = function () {{
    state ^= state << 13; state >>>= 0;
    state ^= state >> 17;
    state ^= state << 5; state >>>= 0;
    return state / 4294967296;
  }};
}})();
"#
    )
}

pub const TIMERS: &str = r#"
(function () {
  var queue = [];
  var nextId = 1;

  globalThis.setTimeout = function (fn, delay) {
    var id = nextId++;
    queue.push({ id: id, fn: fn, at: Number(delay) || 0, args: Array.prototype.slice.call(arguments, 2) });
    return id;
  };

  globalThis.setInterval = function (fn, delay) {
    return globalThis.setTimeout(fn, delay);
  };

  globalThis.clearTimeout = function (id) {
    queue = queue.filter(function (entry) { return entry.id !== id; });
  };

  globalThis.clearInterval = globalThis.clearTimeout;
  globalThis.queueMicrotask = function (fn) { globalThis.setTimeout(fn, 0); };
  globalThis.requestAnimationFrame = function (fn) { return globalThis.setTimeout(fn, 16); };
  globalThis.cancelAnimationFrame = globalThis.clearTimeout;
  globalThis.requestIdleCallback = function (fn) { return globalThis.setTimeout(fn, 1); };
  globalThis.cancelIdleCallback = globalThis.clearTimeout;

  globalThis.__wreRunTimers = function (rounds) {
    var ran = 0;
    for (var round = 0; round < (rounds || 8); round++) {
      var batch = queue.sort(function (a, b) { return a.at - b.at; });
      queue = [];
      if (!batch.length) break;
      for (var i = 0; i < batch.length; i++) {
        try {
          batch[i].fn.apply(null, batch[i].args);
          ran++;
        } catch (error) {
          __wre.push("errors", { where: "timer", text: String(error && error.message || error) });
        }
      }
    }
    return ran;
  };

  globalThis.__wrePendingTimers = function () { return queue.length; };
})();
"#;

pub const CODECS: &str = r#"
(function () {
  var CHARS = "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

  if (typeof globalThis.btoa !== "function") {
    globalThis.btoa = function (input) {
      var text = String(input);
      var out = "";
      for (var block = 0, charCode, i = 0, map = CHARS;
           text.charAt(i | 0) || (map = "=", i % 1);
           out += map.charAt(63 & block >> 8 - i % 1 * 8)) {
        charCode = text.charCodeAt(i += 3 / 4);
        if (charCode > 0xff) throw new Error("btoa: character out of range");
        block = block << 8 | charCode;
      }
      return out;
    };
  }

  if (typeof globalThis.atob !== "function") {
    globalThis.atob = function (input) {
      var text = String(input).replace(/=+$/, "");
      var out = "";
      if (text.length % 4 === 1) throw new Error("atob: bad input length");
      for (var bc = 0, bs = 0, buffer, i = 0;
           (buffer = text.charAt(i++));
           ~buffer && (bs = bc % 4 ? bs * 64 + buffer : buffer, bc++ % 4)
             ? out += String.fromCharCode(255 & bs >> (-2 * bc & 6)) : 0) {
        buffer = CHARS.indexOf(buffer);
      }
      return out;
    };
  }
})();
"#;

pub const ACCESS_TRAP: &str = r#"
globalThis.__wreWatch = function (holder, name, label) {
  var target = holder[name];
  if (target === undefined || target === null) return false;
  var tag = label || name;

  var proxy = new Proxy(target, {
    get: function (object, property, receiver) {
      if (typeof property === "string") {
        __wre.push("access", { on: tag, kind: "get", key: property });
      }
      var value = Reflect.get(object, property, object);
      return typeof value === "function" ? value.bind(object) : value;
    },
    set: function (object, property, value) {
      if (typeof property === "string") {
        __wre.push("access", { on: tag, kind: "set", key: property });
      }
      return Reflect.set(object, property, value);
    },
    has: function (object, property) {
      if (typeof property === "string") {
        __wre.push("access", { on: tag, kind: "has", key: property });
      }
      return Reflect.has(object, property);
    }
  });

  try {
    Object.defineProperty(holder, name, { value: proxy, configurable: true, writable: true });
    return true;
  } catch (error) {
    return false;
  }
};

globalThis.__wreTrace = function (holder, name, label) {
  var original = holder[name];
  if (typeof original !== "function") return false;
  var tag = label || name;

  function traced() {
    var parts = [];
    for (var i = 0; i < arguments.length; i++) parts.push(__wre.describe(arguments[i]));
    var record = { fn: tag, args: parts };
    __wre.push("calls", record);
    try {
      var value = original.apply(this, arguments);
      record.result = __wre.describe(value);
      return value;
    } catch (error) {
      record.threw = String(error && error.message || error);
      throw error;
    }
  }

  try {
    Object.defineProperty(traced, "name", { value: original.name, configurable: true });
    Object.defineProperty(traced, "length", { value: original.length, configurable: true });
    holder[name] = traced;
    return true;
  } catch (error) {
    return false;
  }
};
"#;
