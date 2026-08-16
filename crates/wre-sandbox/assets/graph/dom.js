(function () {
  var env = globalThis.__ENV;
  var bridge = env.bridge;
  var snapshot = env.snapshot;
  var traits = env.traits || {};

  var asMethod = env.asMethod;

  var asNative = function (fn, name) {
    try {
      Object.defineProperty(fn, "name", { value: name, configurable: true });
    } catch (error) {}

    env.sources.set(fn, env.nativeSource(name));
    return fn;
  };

  var shapeOwner = function (target, name) {
    var shapes = env.shapes;
    if (!shapes || !target) return target;
    if (Object.prototype.hasOwnProperty.call(target, name)) return target;

    var current = target;

    for (var steps = 0; steps < 12 && current && current !== Object.prototype && current !== Function.prototype; steps += 1) {
      var constructor = null;

      try {
        constructor = current.constructor;
      } catch (error) {
        constructor = null;
      }

      var owns = false;

      try {
        owns = Boolean(constructor) && constructor.prototype === current;
      } catch (error) {
        owns = false;
      }

      var owned = owns && constructor.name ? shapes[constructor.name] : null;
      if (owned && owned.indexOf(name) !== -1) return current;

      try {
        current = Object.getPrototypeOf(current);
      } catch (error) {
        current = null;
      }
    }

    return target;
  };

  var patch = function (target, name, implementation) {
    if (!target) return;

    target = shapeOwner(target, name);

    var inner = implementation;

    var traced = asMethod(function () {
      env.guard(target, this, name);
      var at = env.count(typeof arguments[0] === "string" ? name + ":" + arguments[0].slice(0, 24) : name);
      var outcome = inner.apply(this, arguments);
      if (at >= 0) env.note(at, arguments, outcome);
      return outcome;
    }, name, inner.length);

    try {
      Object.defineProperty(traced, "length", { value: inner.length, configurable: true });
    } catch (error) {}

    try {
      var descriptor = Object.getOwnPropertyDescriptor(target, name);
      Object.defineProperty(target, name, {
        value: asNative(traced, name),
        writable: descriptor ? descriptor.writable !== false : true,
        enumerable: descriptor ? descriptor.enumerable : true,
        configurable: descriptor ? descriptor.configurable !== false : true,
      });
    } catch (error) {}
  };

  var patchGetter = function (target, name, getter) {
    if (!target) return;

    target = shapeOwner(target, name);

    try {
      var descriptor = Object.getOwnPropertyDescriptor(target, name);
      var tracedGetter = asMethod(function () {
        env.guard(target, this, name);
        var at = env.count("get " + name);
        var outcome = getter.call(this);
        if (at >= 0) env.note(at, null, outcome);
        return outcome;
      }, "get " + name, 0);

      Object.defineProperty(target, name, {
        get: asNative(tracedGetter, "get " + name),
        set: descriptor && descriptor.set ? descriptor.set : undefined,
        enumerable: descriptor ? descriptor.enumerable : true,
        configurable: true,
      });
    } catch (error) {}
  };

  var protoOf = function (value) {
    try {
      return value ? Object.getPrototypeOf(value) : null;
    } catch (error) {
      return null;
    }
  };

  var sampleOf = function (name) {
    var entry = snapshot.samples[name];
    return entry && entry.k === "ref" ? env.materialize(entry.id, "sample:" + name) : null;
  };

  var log = [];
  env.log = log;

  var record = function (kind, detail) {
    log.push({ kind: kind, detail: detail, at: Date.now() });
  };

  env.record = record;

  var utf8Encode = function (input) {
    var text = String(input);
    var bytes = [];

    for (var index = 0; index < text.length; index += 1) {
      var code = text.charCodeAt(index);

      if (code < 0x80) {
        bytes.push(code);
      } else if (code < 0x800) {
        bytes.push(0xc0 | (code >> 6), 0x80 | (code & 63));
      } else if (code >= 0xd800 && code <= 0xdbff && index + 1 < text.length) {
        var next = text.charCodeAt(index + 1);

        if (next >= 0xdc00 && next <= 0xdfff) {
          var point = 0x10000 + ((code - 0xd800) << 10) + (next - 0xdc00);
          bytes.push(0xf0 | (point >> 18), 0x80 | ((point >> 12) & 63), 0x80 | ((point >> 6) & 63), 0x80 | (point & 63));
          index += 1;
        } else {
          bytes.push(0xef, 0xbf, 0xbd);
        }
      } else if (code >= 0xd800 && code <= 0xdfff) {
        bytes.push(0xef, 0xbf, 0xbd);
      } else {
        bytes.push(0xe0 | (code >> 12), 0x80 | ((code >> 6) & 63), 0x80 | (code & 63));
      }
    }

    return Uint8Array.from(bytes);
  };

  var utf8Decode = function (bytes) {
    var view = bytes instanceof Uint8Array ? bytes : new Uint8Array(bytes && bytes.buffer ? bytes.buffer : bytes || []);
    var out = "";

    for (var index = 0; index < view.length; ) {
      var byte = view[index];

      if (byte < 0x80) {
        out += String.fromCharCode(byte);
        index += 1;
      } else if (byte < 0xe0) {
        out += String.fromCharCode(((byte & 31) << 6) | (view[index + 1] & 63));
        index += 2;
      } else if (byte < 0xf0) {
        out += String.fromCharCode(((byte & 15) << 12) | ((view[index + 1] & 63) << 6) | (view[index + 2] & 63));
        index += 3;
      } else {
        var point = ((byte & 7) << 18) | ((view[index + 1] & 63) << 12) | ((view[index + 2] & 63) << 6) | (view[index + 3] & 63);
        point -= 0x10000;
        out += String.fromCharCode(0xd800 + (point >> 10), 0xdc00 + (point & 1023));
        index += 4;
      }
    }

    return out;
  };

  if (env.captureCipher) {
    var realJoin = Array.prototype.join;

    try {
      Object.defineProperty(Array.prototype, "join", {
        value: asNative(function join() {
          var out = realJoin.apply(this, arguments);
          if (typeof out === "string" && out.length > 5000) record("payload", out);
          return out;
        }, "join"),
        writable: true,
        enumerable: false,
        configurable: true,
      });
    } catch (error) {}
  }

  if (env.captureCipher) {
    var realFreeze = Object.freeze;

    try {
      Object.defineProperty(Object, "freeze", {
        value: asNative(function freeze(target) {
          try {
            if (Array.isArray(target) && target.length > 100) {
              var graft = env.vectorGraft;

              if (graft && graft.values && graft.values.length === target.length) {
                for (var slot = 0; slot < graft.slots.length; slot += 1) {
                  var index = graft.slots[slot];
                  if (index >= 2 && index < target.length) target[index] = graft.values[index];
                }
              }

              record("vector", env.rawStringify(target));
            }
          } catch (error) {}

          return realFreeze(target);
        }, "freeze"),
        writable: true,
        enumerable: false,
        configurable: true,
      });
    } catch (error) {}
  }

  var realStringify = JSON.stringify;

  try {
    Object.defineProperty(JSON, "stringify", {
      value: asNative(function stringify() {
        var out = realStringify.apply(JSON, arguments);
        if (typeof out === "string" && out.length > 2000) record("payload", out);
        return out;
      }, "stringify"),
      writable: true,
      enumerable: false,
      configurable: true,
    });
  } catch (error) {}

  var TextEncoderShim = function TextEncoder() {};

  TextEncoderShim.prototype.encode = asNative(function encode(input) {
    if (typeof input === "string" && input.length > 2000) record("payload", input);
    return utf8Encode(input === undefined ? "" : input);
  }, "encode");

  TextEncoderShim.prototype.encodeInto = asNative(function encodeInto(input, target) {
    var bytes = utf8Encode(input);
    target.set(bytes.subarray(0, target.length));
    return { read: String(input).length, written: Math.min(bytes.length, target.length) };
  }, "encodeInto");

  Object.defineProperty(TextEncoderShim.prototype, "encoding", {
    get: asNative(function () { return "utf-8"; }, "get encoding"),
    configurable: true,
  });

  globalThis.TextEncoder = asNative(TextEncoderShim, "TextEncoder");

  var TextDecoderShim = function TextDecoder() {};

  TextDecoderShim.prototype.decode = asNative(function decode(input) {
    return input === undefined ? "" : utf8Decode(input);
  }, "decode");

  Object.defineProperty(TextDecoderShim.prototype, "encoding", {
    get: asNative(function () { return "utf-8"; }, "get encoding"),
    configurable: true,
  });

  globalThis.TextDecoder = asNative(TextDecoderShim, "TextDecoder");

  var ALPHABET = "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

  globalThis.btoa = asNative(function btoa(input) {
    var text = String(input);
    var out = "";

    for (var index = 0; index < text.length; index += 3) {
      var a = text.charCodeAt(index);
      var b = text.charCodeAt(index + 1);
      var c = text.charCodeAt(index + 2);
      var triple = (a << 16) | ((isNaN(b) ? 0 : b) << 8) | (isNaN(c) ? 0 : c);

      out += ALPHABET[(triple >> 18) & 63] + ALPHABET[(triple >> 12) & 63];
      out += isNaN(b) ? "=" : ALPHABET[(triple >> 6) & 63];
      out += isNaN(c) ? "=" : ALPHABET[triple & 63];
    }

    return out;
  }, "btoa");

  globalThis.atob = asNative(function atob(input) {
    var text = String(input).replace(/=+$/, "");
    var out = "";
    var buffer = 0;
    var bits = 0;

    for (var index = 0; index < text.length; index += 1) {
      var value = ALPHABET.indexOf(text[index]);
      if (value === -1) continue;

      buffer = (buffer << 6) | value;
      bits += 6;

      if (bits >= 8) {
        bits -= 8;
        out += String.fromCharCode((buffer >> bits) & 255);
      }
    }

    return out;
  }, "atob");

  var consoleReads = traits.consoleReads || {};

  var readLikeConsole = function (level, argv) {
    var counts = consoleReads[level];
    if (!counts) return;

    for (var index = 0; index < argv.length; index += 1) {
      var value = argv[index];
      if (!(value instanceof Error)) continue;

      for (var stackRead = 0; stackRead < counts[0]; stackRead += 1) void value.stack;
      for (var nameRead = 0; nameRead < counts[1]; nameRead += 1) void value.name;
      for (var messageRead = 0; messageRead < counts[2]; messageRead += 1) void value.message;
    }
  };

  if (typeof globalThis.console !== "object" || globalThis.console === null) {
    globalThis.console = {};
    var levels = ["log", "warn", "error", "info", "debug", "trace", "table", "dir", "group", "groupEnd", "time", "timeEnd", "assert", "count", "clear"];

    for (var l = 0; l < levels.length; l += 1) {
      (function (level) {
        globalThis.console[level] = asNative(function () {
          readLikeConsole(level, arguments);
        }, level);
      })(levels[l]);
    }
  }

  var hexOf = function (value) {
    var view = value instanceof Uint8Array ? value : new Uint8Array(value.buffer || value);
    var out = "";

    for (var index = 0; index < view.length; index += 1) {
      out += (view[index] < 16 ? "0" : "") + view[index].toString(16);
    }

    return out;
  };

  var isBytes = function (value) {
    return value instanceof Uint8Array || (value && typeof value === "object" && typeof value.byteLength === "number" && typeof value.length === "number");
  };

  if (env.debugFunctions) {
    var RealFunction = globalThis.Function;
    env.functionSources = [];

    globalThis.__KT = {
      get: function (receiver, key) {
        if (receiver === undefined || receiver === null) {
          env.count("read " + String(key) + " of " + String(receiver));
          return undefined;
        }

        return receiver[key];
      },
      glob: function (key) {
        var value = globalThis[key];
        if (value === undefined) env.count("missing global " + String(key));
        return value;
      },
    };

    var watch = function (fn) {
      if (!env.captureCipher || typeof fn !== "function") return fn;

      return new Proxy(fn, {
        apply: function (target, self, args) {
          for (var index = 0; index < args.length; index += 1) {
            var value = args[index];

            if (typeof value === "string" && value.length > 1000) {
              record("payload", value);
              break;
            }

            if (value && typeof value === "object" && typeof value.length === "number" && value.length > 1000) {
              record("cipher", {
                position: index,
                arity: args.length,
                key: index > 0 && args[0] && args[0].length ? hexOf(args[0]) : null,
                iv: index > 1 && args[1] && args[1].length ? hexOf(args[1]) : null,
                data: hexOf(value),
              });
              break;
            }
          }

          return Reflect.apply(target, self, args);
        },
      });
    };

    var rewrite = function (source) {
      if (!env.debugFunctions) return source;

      return String(source)
        .replace(/i\[0\]\s*\[\s*e\(n\)\s*\]/g, "__KT.glob(e(n))")
        .replace(/e\(n\)\s*\[\s*e\(n\)\s*\]\s*=/g, "__KT.get(e(n),e(n))=")
        .replace(/e\(n\)\s*\[\s*e\(n\)\s*\]/g, "__KT.get(e(n),e(n))")
        .replace(/__KT\.get\(e\(n\),e\(n\)\)=/g, "e(n)[e(n)]=");
    };

    var rewriteArgs = function (args) {
      var copy = Array.prototype.slice.call(args);
      if (copy.length) copy[copy.length - 1] = rewrite(copy[copy.length - 1]);
      env.functionSources.push(copy[copy.length - 1]);
      return copy;
    };

    var functionProxy = new Proxy(RealFunction, {
      construct: function (target, args) {
        return watch(Reflect.construct(target, rewriteArgs(args)));
      },
      apply: function (target, self, args) {
        return watch(target.apply(self, rewriteArgs(args)));
      },
    });

    globalThis.Function = functionProxy;

    try {
      Object.defineProperty(Function.prototype, "constructor", {
        value: functionProxy,
        writable: true,
        enumerable: false,
        configurable: true,
      });
    } catch (error) {}
  }

  var timers = new Map();
  var nextTimer = 1;

  var schedule = function (label, callback, delay, extra) {
    var id = nextTimer++;
    env.count(label + ":" + String(typeof delay === "number" ? delay : 0));
    timers.set(id, callback);
    bridge.schedule(id, typeof delay === "number" ? delay : 0, extra);
    return id;
  };

  globalThis.setTimeout = asNative(function setTimeout(callback, delay) {
    return schedule("setTimeout", callback, delay, Array.prototype.slice.call(arguments, 2));
  }, "setTimeout");

  globalThis.setInterval = asNative(function setInterval(callback, delay) {
    return schedule("setInterval", callback, delay, Array.prototype.slice.call(arguments, 2));
  }, "setInterval");

  globalThis.clearTimeout = asNative(function clearTimeout(id) {
    timers.delete(id);
    bridge.unschedule(id);
  }, "clearTimeout");

  globalThis.clearInterval = asNative(function clearInterval(id) {
    return globalThis.clearTimeout(id);
  }, "clearInterval");

  var noiseSeed = 0x2545f491;

  var noise = function () {
    noiseSeed ^= noiseSeed << 13;
    noiseSeed ^= noiseSeed >>> 17;
    noiseSeed ^= noiseSeed << 5;
    return (noiseSeed >>> 0) / 4294967296;
  };

  var noiseUuid = function () {
    var digits = "0123456789abcdef";
    var out = "";

    for (var index = 0; index < 36; index += 1) {
      if (index === 8 || index === 13 || index === 18 || index === 23) {
        out += "-";
        continue;
      }

      if (index === 14) {
        out += "4";
        continue;
      }

      var value = Math.floor(noise() * 16);
      if (index === 19) value = (value & 3) | 8;
      out += digits.charAt(value);
    }

    return out;
  };

  var nextInternal = -1;

  var later = function (callback, delay) {
    var id = nextInternal;
    nextInternal -= 1;
    timers.set(id, callback);
    bridge.schedule(id, delay, []);
    return id;
  };

  env.later = later;

  var frameCallbacks = new Map();
  var idleCallbacks = new Map();
  var nextFrameCallback = 1;
  var nextIdleCallback = 1;

  globalThis.requestAnimationFrame = asNative(function requestAnimationFrame(callback) {
    var id = nextFrameCallback;
    nextFrameCallback += 1;

    frameCallbacks.set(id, later(function () {
      frameCallbacks.delete(id);
      callback(env.clock());
    }, 16));

    return id;
  }, "requestAnimationFrame");

  globalThis.cancelAnimationFrame = asNative(function cancelAnimationFrame(id) {
    var timer = frameCallbacks.get(id);
    if (timer === undefined) return undefined;
    frameCallbacks.delete(id);
    timers.delete(timer);
    bridge.unschedule(timer);
    return undefined;
  }, "cancelAnimationFrame");

  globalThis.requestIdleCallback = asNative(function requestIdleCallback(callback) {
    var id = nextIdleCallback;
    nextIdleCallback += 1;

    idleCallbacks.set(id, later(function () {
      idleCallbacks.delete(id);
      callback({ didTimeout: false, timeRemaining: function () { return 12; } });
    }, 24));

    return id;
  }, "requestIdleCallback");

  globalThis.cancelIdleCallback = asNative(function cancelIdleCallback(id) {
    var timer = idleCallbacks.get(id);
    if (timer === undefined) return undefined;
    idleCallbacks.delete(id);
    timers.delete(timer);
    bridge.unschedule(timer);
    return undefined;
  }, "cancelIdleCallback");

  globalThis.queueMicrotask = asNative(function queueMicrotask(callback) {
    Promise.resolve().then(callback);
  }, "queueMicrotask");

  env.fire = function (id, extra) {
    var callback = timers.get(id);
    if (!callback) return false;
    timers.delete(id);

    if (typeof callback === "function") callback.apply(null, extra || []);
    return true;
  };

  env.pendingTimers = function () {
    return timers.size;
  };

  var listeners = new Map();

  var listenersFor = function (target, type) {
    var byType = listeners.get(target);

    if (!byType) {
      byType = new Map();
      listeners.set(target, byType);
    }

    var list = byType.get(type);

    if (!list) {
      list = [];
      byType.set(type, list);
    }

    return list;
  };

  var addListener = function addEventListener(type, handler) {
    if (typeof handler !== "function" && (!handler || typeof handler.handleEvent !== "function")) return undefined;
    listenersFor(this, String(type)).push(handler);
    return undefined;
  };

  var removeListener = function removeEventListener(type, handler) {
    var list = listenersFor(this, String(type));
    var index = list.indexOf(handler);
    if (index !== -1) list.splice(index, 1);
    return undefined;
  };

  var dispatch = function (target, event) {
    var type = event.type;
    var bubbles = event.bubbles;
    var cancelable = event.cancelable;
    var composed = event.composed;
    var list = listenersFor(target, type).slice();
    var fields = eventFields.get(event);

    if (fields) {
      fields.target = fields.target || target;
      fields.currentTarget = target;
      fields.srcElement = fields.target;
    } else {
      event.target = event.target || target;
      event.currentTarget = target;
    }

    for (var index = 0; index < list.length; index += 1) {
      var handler = list[index];

      try {
        if (typeof handler === "function") handler.call(target, event);
        else handler.handleEvent(event);
      } catch (error) {
        record("listenerError", { type: event.type, message: String(error && error.message) });
      }
    }

    var inline = target["on" + type];

    if (typeof inline === "function") {
      try {
        inline.call(target, event);
      } catch (error) {
        record("listenerError", { type: event.type, message: String(error && error.message) });
      }
    }

    return true;
  };

  env.dispatch = dispatch;

  var makeEvent = function (type, extra) {
    var event = { type: String(type), bubbles: false, cancelable: false, timeStamp: env.clock(), isTrusted: true, target: null, currentTarget: null };
    if (extra) for (var key in extra) event[key] = extra[key];
    event.preventDefault = function () {};
    event.stopPropagation = function () {};
    event.stopImmediatePropagation = function () {};
    return event;
  };

  env.makeEvent = makeEvent;

  var eventFields = new WeakMap();

  var eventInitMembers = function (name) {
    var recorded = traits.eventInit && traits.eventInit[name];
    if (!recorded) return null;

    var members = [];

    for (var index = 0; index < recorded.length; index += 1) {
      if (recorded[index].indexOf("g:") === 0) members.push(recorded[index].slice(2));
    }

    return members.length ? members : null;
  };

  var EVENT_CONSTRUCTORS = [
    "Event", "CustomEvent", "UIEvent", "MouseEvent", "PointerEvent", "KeyboardEvent", "WheelEvent",
    "FocusEvent", "InputEvent", "CompositionEvent", "MessageEvent", "ErrorEvent", "PromiseRejectionEvent",
    "ProgressEvent", "StorageEvent", "PageTransitionEvent", "HashChangeEvent", "PopStateEvent",
    "AnimationEvent", "TransitionEvent", "ClipboardEvent", "DragEvent", "SubmitEvent", "ToggleEvent",
    "SecurityPolicyViolationEvent", "GamepadEvent", "DeviceMotionEvent", "DeviceOrientationEvent",
    "TouchEvent", "MediaQueryListEvent",
  ];

  var eventDefaults = {
    MouseEvent: { screenX: 0, screenY: 0, clientX: 0, clientY: 0, pageX: 0, pageY: 0, offsetX: 0, offsetY: 0, movementX: 0, movementY: 0, button: 0, buttons: 0, ctrlKey: false, shiftKey: false, altKey: false, metaKey: false, relatedTarget: null, detail: 0, view: null },
    PointerEvent: { pointerId: 0, width: 1, height: 1, pressure: 0, tangentialPressure: 0, tiltX: 0, tiltY: 0, twist: 0, pointerType: "", isPrimary: false, clientX: 0, clientY: 0, screenX: 0, screenY: 0, button: 0, buttons: 0, detail: 0, view: null },
    KeyboardEvent: { key: "", code: "", location: 0, repeat: false, isComposing: false, charCode: 0, keyCode: 0, which: 0, ctrlKey: false, shiftKey: false, altKey: false, metaKey: false, detail: 0, view: null },
    CustomEvent: { detail: null },
    UIEvent: { detail: 0, view: null },
    MessageEvent: { data: null, origin: "", lastEventId: "", source: null, ports: [] },
    ProgressEvent: { lengthComputable: false, loaded: 0, total: 0 },
    ErrorEvent: { message: "", filename: "", lineno: 0, colno: 0, error: null },
  };

  for (var eventIndex = 0; eventIndex < EVENT_CONSTRUCTORS.length; eventIndex += 1) {
    (function (name) {
      if (typeof globalThis[name] !== "function") return;

      var makeFields = function (type, init) {
        var fields = {
          type: String(type),
          bubbles: false,
          cancelable: false,
          composed: false,
          defaultPrevented: false,
          eventPhase: 0,
          isTrusted: false,
          timeStamp: env.clock(),
          target: null,
          currentTarget: null,
          srcElement: null,
          returnValue: true,
          cancelBubble: false,
          NONE: 0,
          CAPTURING_PHASE: 1,
          AT_TARGET: 2,
          BUBBLING_PHASE: 3,
        };

        var defaults = eventDefaults[name];
        if (defaults) for (var key in defaults) fields[key] = defaults[key];

        var members = eventInitMembers(name);

        if (init !== null && init !== undefined && (typeof init === "object" || typeof init === "function")) {
          if (members) {
            for (var at = 0; at < members.length; at += 1) {
              var value = init[members[at]];
              if (value !== undefined) fields[members[at]] = value;
            }
          } else {
            for (var given in init) fields[given] = init[given];
          }
        }

        return fields;
      };

      var Original = globalThis[name];

      var Shell = function (type, init) {
        if (new.target === undefined) {
          throw env.hideFrames(new TypeError("Failed to construct '" + name + "': Please use the 'new' operator, this DOM object constructor cannot be called as a function."));
        }

        if (type === undefined) {
          throw env.hideFrames(new TypeError("Failed to construct '" + name + "': 1 argument required, but only 0 present."));
        }

        var made = Object.create(Original.prototype);
        var fields = makeFields(type, init);

        env.overrides.set(made, fields);
        eventFields.set(made, fields);

        try {
          Object.defineProperty(made, "preventDefault", { value: asNative(function preventDefault() { fields.defaultPrevented = Boolean(fields.cancelable); }, "preventDefault"), writable: true, enumerable: false, configurable: true });
          Object.defineProperty(made, "stopPropagation", { value: asNative(function stopPropagation() { fields.cancelBubble = true; }, "stopPropagation"), writable: true, enumerable: false, configurable: true });
          Object.defineProperty(made, "stopImmediatePropagation", { value: asNative(function stopImmediatePropagation() { fields.cancelBubble = true; }, "stopImmediatePropagation"), writable: true, enumerable: false, configurable: true });
        } catch (error) {}

        return made;
      };

      asNative(Shell, name);

      try {
        Object.defineProperty(Shell, "prototype", { value: Original.prototype, writable: false, enumerable: false, configurable: false });
        Object.defineProperty(Shell, "length", { value: 1, configurable: true });
        Object.defineProperty(Original.prototype, "constructor", { value: Shell, writable: true, enumerable: false, configurable: true });
        Object.defineProperty(globalThis, name, { value: Shell, writable: true, enumerable: false, configurable: true });
      } catch (error) {}
    })(EVENT_CONSTRUCTORS[eventIndex]);
  }

  (function installBlob() {
    var shape = traits.blob;
    var Original = globalThis.Blob;

    if (!shape || typeof Original !== "function" || !Original.prototype) return;

    var Shell = function (parts, options) {
      if (new.target === undefined) {
        throw env.hideFrames(new TypeError("Failed to construct 'Blob': Please use the 'new' operator, this DOM object constructor cannot be called as a function."));
      }

      if (parts !== undefined) {
        var iterator = parts === null ? undefined : parts[Symbol.iterator];

        if (typeof iterator !== "function") {
          throw env.hideFrames(new TypeError(shape.thrown ? shape.thrown.message : "Failed to construct 'Blob': The object must have a callable @@iterator property."));
        }
      }

      if (options !== null && options !== undefined && (typeof options === "object" || typeof options === "function")) {
        var members = shape.options || [];

        for (var index = 0; index < members.length; index += 1) {
          if (members[index].indexOf("g:") === 0) void options[members[index].slice(2)];
        }
      }

      return Reflect.construct(Original, arguments, new.target);
    };

    asNative(Shell, "Blob");

    try {
      Object.defineProperty(Shell, "prototype", { value: Original.prototype, writable: false, enumerable: false, configurable: false });
      Object.defineProperty(Shell, "length", { value: 0, configurable: true });
      Object.defineProperty(Original.prototype, "constructor", { value: Shell, writable: true, enumerable: false, configurable: true });
      Object.defineProperty(globalThis, "Blob", { value: Shell, writable: true, enumerable: false, configurable: true });
    } catch (error) {}
  })();

  var COMMON_EVENT_FIELDS = [
    "type", "bubbles", "cancelable", "composed", "defaultPrevented", "eventPhase", "isTrusted",
    "timeStamp", "target", "currentTarget", "srcElement", "returnValue", "cancelBubble",
  ];

  var defineEventFields = function (name, fields) {
    var holder = globalThis[name] && globalThis[name].prototype;
    if (!holder) return;

    for (var index = 0; index < fields.length; index += 1) {
      (function (field) {
        try {
          Object.defineProperty(holder, field, {
            get: asNative(function () {
              var state = eventFields.get(this);
              return state ? state[field] : undefined;
            }, "get " + field),
            enumerable: true,
            configurable: true,
          });
        } catch (error) {}
      })(fields[index]);
    }
  };

  defineEventFields("Event", COMMON_EVENT_FIELDS);

  for (var defaultsName in eventDefaults) defineEventFields(defaultsName, Object.keys(eventDefaults[defaultsName]));

  env.eventFields = eventFields;

  var document = globalThis.document;
  var Document = globalThis.Document;
  var Element = globalThis.Element;
  var HTMLElement = globalThis.HTMLElement;
  var Node = protoOf(Element && Element.prototype) ? (Element.prototype && protoOf(Element.prototype).constructor) : null;
  var EventTargetPrototype = Node && Node.prototype ? protoOf(Node.prototype) : null;

  var targets = [EventTargetPrototype || (Document && Document.prototype), globalThis];

  for (var t = 0; t < targets.length; t += 1) {
    if (!targets[t]) continue;
    patch(targets[t], "addEventListener", addListener);
    patch(targets[t], "removeEventListener", removeListener);
    patch(targets[t], "dispatchEvent", function dispatchEvent(event) { return dispatch(this, event); });
  }

  var dashedName = function (key) {
    return String(key).replace(/^webkit/, "-webkit-").replace(/([A-Z])/g, function (match) { return "-" + match.toLowerCase(); });
  };

  var camelNames = null;
  var camelSet = null;
  var camelEnumerable = null;

  var styleNames = function () {
    if (camelNames !== null) return camelNames;

    var shape = env.styleShape;
    camelNames = shape && shape.camel && shape.camel.length ? shape.camel : [];
    camelSet = Object.create(null);
    camelEnumerable = Object.create(null);

    for (var index = 0; index < camelNames.length; index += 1) camelSet[camelNames[index]] = true;

    var enumerable = shape && Array.isArray(shape.keys) ? shape.keys : camelNames;
    for (var at = 0; at < enumerable.length; at += 1) camelEnumerable[enumerable[at]] = true;

    return camelNames;
  };

  var styleProtoOf = function () {
    try {
      return globalThis.CSSStyleDeclaration ? globalThis.CSSStyleDeclaration.prototype : null;
    } catch (error) {
      return null;
    }
  };

  var styleOwners = new WeakMap();

  var makeStyle = function (owner) {
    var values = Object.create(null);
    var order = [];

    var write = function (key, value) {
      var name = dashedName(key);
      var text = value === null || value === undefined ? "" : String(value);

      if (!text) {
        if (name in values) {
          delete values[name];
          order.splice(order.indexOf(name), 1);
        }

        return;
      }

      if (!(name in values)) order.push(name);
      values[name] = text;
    };

    var cssText = function () {
      var parts = [];
      for (var index = 0; index < order.length; index += 1) parts.push(order[index] + ": " + values[order[index]] + ";");
      return parts.join(" ");
    };

    var members = {
      getPropertyValue: asNative(function getPropertyValue(key) { return values[dashedName(key)] || ""; }, "getPropertyValue"),
      getPropertyPriority: asNative(function getPropertyPriority() { return ""; }, "getPropertyPriority"),
      setProperty: asNative(function setProperty(key, value) { write(key, value); }, "setProperty"),
      removeProperty: asNative(function removeProperty(key) {
        var name = dashedName(key);
        var had = values[name] || "";
        write(name, "");
        return had;
      }, "removeProperty"),
      item: asNative(function item(index) { return order[Number(index)] || ""; }, "item"),
    };

    return new Proxy(members, {
      get: function (target, key) {
        if (key === "length") return order.length;
        if (key === "cssText") return cssText();
        if (key === "parentRule") return null;
        if (key in target) return target[key];
        if (typeof key !== "string") return undefined;
        if (/^\d+$/.test(key)) return order[Number(key)];
        return values[dashedName(key)] || "";
      },
      set: function (target, key, value) {
        if (key === "cssText") {
          order.length = 0;
          for (var name in values) delete values[name];

          try {
            var styleOwner = owner;
            if (styleOwner) {
              var attrs = attributesOf(styleOwner);
              attrs.style = String(value);
              env.syncAttributes(styleOwner);
            }
          } catch (error) {}

          var declarations = String(value).split(";");

          for (var index = 0; index < declarations.length; index += 1) {
            var pair = declarations[index].split(":");
            if (pair.length < 2) continue;
            write(pair[0].trim(), pair.slice(1).join(":").trim());
          }

          return true;
        }

        write(key, value);
        return true;
      },
      has: function (target, key) {
        if (key === "length" || key === "cssText" || key === "parentRule") return true;
        if (key in target) return true;
        return typeof key === "string" && dashedName(key) in values;
      },
      getPrototypeOf: function () {
        return styleProtoOf() || Object.prototype;
      },
      ownKeys: function (target) {
        var camel = styleNames();
        if (!camel.length) return order.concat(["length", "cssText"]);

        var keys = [];
        for (var index = 0; index < order.length; index += 1) keys.push(String(index));
        return keys.concat(camel);
      },
      getOwnPropertyDescriptor: function (target, key) {
        var camel = styleNames();

        if (!camel.length) {
          if (key === "length") return { value: order.length, writable: false, enumerable: true, configurable: true };
          if (key === "cssText") return { value: cssText(), writable: true, enumerable: true, configurable: true };
          if (typeof key === "string" && dashedName(key) in values) {
            return { value: values[dashedName(key)], writable: true, enumerable: true, configurable: true };
          }

          return Object.getOwnPropertyDescriptor(target, key);
        }

        if (typeof key !== "string") return undefined;

        if (/^\d+$/.test(key)) {
          if (Number(key) >= order.length) return undefined;
          return { value: order[Number(key)], writable: false, enumerable: true, configurable: true };
        }

        if (!camelSet[key]) return undefined;
        return { value: values[dashedName(key)] || "", writable: true, enumerable: camelEnumerable[key] === true, configurable: true };
      },
    });
  };

  var canvasSample = sampleOf("document.createElement(canvas)");
  var divSample = sampleOf("document.createElement(div)");
  var iframeSample = sampleOf("document.createElement(iframe)");
  var videoSample = sampleOf("document.createElement(video)");
  var audioSample = sampleOf("document.createElement(audio)");

  var INTERFACE_FOR_TAG = {
    a: "HTMLAnchorElement", area: "HTMLAreaElement", audio: "HTMLAudioElement", base: "HTMLBaseElement",
    blockquote: "HTMLQuoteElement", body: "HTMLBodyElement", br: "HTMLBRElement", button: "HTMLButtonElement",
    canvas: "HTMLCanvasElement", caption: "HTMLTableCaptionElement", col: "HTMLTableColElement",
    colgroup: "HTMLTableColElement", data: "HTMLDataElement", datalist: "HTMLDataListElement",
    del: "HTMLModElement", details: "HTMLDetailsElement", dialog: "HTMLDialogElement", dir: "HTMLDirectoryElement",
    div: "HTMLDivElement", dl: "HTMLDListElement", embed: "HTMLEmbedElement", fieldset: "HTMLFieldSetElement",
    font: "HTMLFontElement", form: "HTMLFormElement", frame: "HTMLFrameElement", frameset: "HTMLFrameSetElement",
    h1: "HTMLHeadingElement", h2: "HTMLHeadingElement", h3: "HTMLHeadingElement", h4: "HTMLHeadingElement",
    h5: "HTMLHeadingElement", h6: "HTMLHeadingElement", head: "HTMLHeadElement", hr: "HTMLHRElement",
    html: "HTMLHtmlElement", iframe: "HTMLIFrameElement", img: "HTMLImageElement", input: "HTMLInputElement",
    ins: "HTMLModElement", label: "HTMLLabelElement", legend: "HTMLLegendElement", li: "HTMLLIElement",
    link: "HTMLLinkElement", map: "HTMLMapElement", marquee: "HTMLMarqueeElement", menu: "HTMLMenuElement",
    meta: "HTMLMetaElement", meter: "HTMLMeterElement", object: "HTMLObjectElement", ol: "HTMLOListElement",
    optgroup: "HTMLOptGroupElement", option: "HTMLOptionElement", output: "HTMLOutputElement",
    p: "HTMLParagraphElement", param: "HTMLParamElement", picture: "HTMLPictureElement", pre: "HTMLPreElement",
    progress: "HTMLProgressElement", q: "HTMLQuoteElement", script: "HTMLScriptElement", select: "HTMLSelectElement",
    slot: "HTMLSlotElement", source: "HTMLSourceElement", span: "HTMLSpanElement", style: "HTMLStyleElement",
    table: "HTMLTableElement", tbody: "HTMLTableSectionElement", td: "HTMLTableCellElement",
    template: "HTMLTemplateElement", textarea: "HTMLTextAreaElement", tfoot: "HTMLTableSectionElement",
    th: "HTMLTableCellElement", thead: "HTMLTableSectionElement", time: "HTMLTimeElement", title: "HTMLTitleElement",
    tr: "HTMLTableRowElement", track: "HTMLTrackElement", ul: "HTMLUListElement", video: "HTMLVideoElement",
    xmp: "HTMLPreElement",
  };

  var PLAIN_TAGS = ("abbr address article aside b bdi bdo cite code dd dfn dt em figcaption figure footer header " +
    "hgroup i kbd main mark nav noscript rp rt ruby s samp search section small strong sub summary sup u var wbr " +
    "acronym big center nobr plaintext strike tt").split(" ");

  var prototypeFor = function (tag) {
    var name = INTERFACE_FOR_TAG[tag];

    if (!name) {
      if (PLAIN_TAGS.indexOf(tag) !== -1 || tag.indexOf("-") !== -1) name = "HTMLElement";
      else name = "HTMLUnknownElement";
    }

    var constructor = globalThis[name];
    if (constructor && constructor.prototype) return constructor.prototype;

    var sample =
      tag === "canvas" ? canvasSample :
      tag === "iframe" ? iframeSample :
      tag === "video" ? videoSample :
      tag === "audio" ? audioSample :
      divSample;

    return sample ? protoOf(sample) : (globalThis.HTMLElement && globalThis.HTMLElement.prototype) || Object.prototype;
  };

  var elementCount = 0;
  var canvasCount = 0;

  var elementState = new WeakMap();

  var nodeState = function (node) {
    try {
      return elementState.get(node) || null;
    } catch (error) {
      return null;
    }
  };

  env.nodeState = nodeState;

  var NODE_FIELDS = [
    "nodeName", "nodeType", "parentNode", "parentElement", "firstChild", "lastChild", "nextSibling",
    "previousSibling", "ownerDocument", "textContent", "childNodes",
  ];

  var ELEMENT_FIELDS = [
    "tagName", "localName", "className", "id", "children", "attributes", "outerHTML",
    "firstElementChild", "lastElementChild", "childElementCount", "previousElementSibling", "nextElementSibling",
  ];
  var HTML_FIELDS = ["style", "innerText", "offsetWidth", "offsetHeight"];

  var inherited = function (previous, self) {
    if (!previous) return undefined;
    if (previous.get) return previous.get.call(self);
    return previous.value;
  };

  var installFields = function (prototype, fields) {
    if (!prototype) return;

    for (var index = 0; index < fields.length; index += 1) {
      (function (field) {
        try {
          var previous = Object.getOwnPropertyDescriptor(prototype, field);

          Object.defineProperty(prototype, field, {
            get: asNative(function () {
              var state = nodeState(this);
              if (state) return state[field];
              return inherited(previous, this);
            }, "get " + field),
            set: asNative(function (value) {
              var state = nodeState(this);
              if (state) state[field] = value;
              else if (previous && previous.set) previous.set.call(this, value);
            }, "set " + field),
            enumerable: previous ? previous.enumerable : true,
            configurable: true,
          });
        } catch (error) {}
      })(fields[index]);
    }
  };

  var layoutTable = null;
  var layoutMisses = [];

  var layoutKey = function (element) {
    var state = nodeState(element);
    if (!state) return null;

    var css = "";

    try {
      css = String(state.style.cssText || "");
    } catch (error) {}

    var attrs = [];

    try {
      var names = Object.keys(state.attrs || {}).sort();

      for (var index = 0; index < names.length; index += 1) {
        if (names[index] === "style") continue;
        attrs.push(names[index] + "=" + String(state.attrs[names[index]]).slice(0, 60));
      }
    } catch (error) {}

    var value = "";

    try {
      if (typeof element.value === "string") value = element.value.slice(0, 80);
    } catch (error) {}

    var text = String(state.html || state.textContent || "").slice(0, 400);
    return state.tag + "|" + css + "|" + attrs.join(" ") + "|" + value + "|" + text;
  };

  var noteLayout = function (key) {
    if (layoutMisses.length < 400 && layoutMisses.indexOf(key) === -1) layoutMisses.push(key);
  };

  env.boxOf = function (element) {
    var empty = { x: 0, y: 0, width: 0, height: 0 };
    var key = layoutKey(element);
    if (!key) return empty;

    noteLayout(key);
    if (!layoutTable) layoutTable = env.layout || null;
    var found = layoutTable ? layoutTable[key] : null;

    if (!found) return empty;

    return { x: found.x || 0, y: found.y || 0, width: found.width || 0, height: found.height || 0 };
  };

  var viewportFacts = env.viewport || null;

  var HIDDEN_TAGS = { script: 1, style: 1, link: 1, meta: 1, title: 1, head: 1, base: 1, template: 1, noscript: 1 };

  var INLINE_TAGS = {
    a: 1, abbr: 1, audio: 1, b: 1, bdi: 1, bdo: 1, br: 1, button: 1, canvas: 1, cite: 1, code: 1, data: 1,
    del: 1, dfn: 1, em: 1, embed: 1, i: 1, iframe: 1, img: 1, input: 1, ins: 1, kbd: 1, label: 1, map: 1,
    mark: 1, meter: 1, object: 1, output: 1, picture: 1, progress: 1, q: 1, ruby: 1, s: 1, samp: 1,
    select: 1, slot: 1, small: 1, span: 1, strong: 1, sub: 1, sup: 1, textarea: 1, time: 1, u: 1,
    "var": 1, video: 1, wbr: 1,
  };

  var strutHeight = traits.bodyStrut && typeof traits.bodyStrut.withInline === "number" ? traits.bodyStrut.withInline : 0;

  env.sizeOf = function (element, which) {
    if (viewportFacts) {
      var owner = nodeState(element);
      var recorded = owner && owner.tag === "html" ? viewportFacts.documentElement : owner && owner.tag === "body" ? viewportFacts.body : null;

      if (recorded && typeof recorded[which] === "number") {
        if (which !== "clientHeight" && which !== "scrollHeight") return recorded[which];
        if (recorded[which] > 0) return recorded[which];
      }
    }

    var key = layoutKey(element);
    if (!key) return 0;

    noteLayout(key);
    if (!layoutTable) layoutTable = env.layout || null;
    var found = layoutTable ? layoutTable[key] : null;

    var state = nodeState(element);
    var stacks = state && (state.tag === "body" || state.tag === "html");

    if (found && !(stacks && !found[which] && (which === "clientHeight" || which === "scrollHeight"))) {
      return found[which] || 0;
    }

    if (!stacks || (which !== "clientHeight" && which !== "scrollHeight")) return 0;

    var children = state.childNodes;
    if (!Array.isArray(children)) return 0;

    var total = 0;
    var inline = false;

    for (var index = 0; index < children.length; index += 1) {
      var child = children[index];
      var childState = nodeState(child);
      if (!childState || HIDDEN_TAGS[childState.tag]) continue;

      var display = "";

      try {
        display = String(childState.style.display || "");
      } catch (error) {}

      if (display === "none") continue;

      if (INLINE_TAGS[childState.tag] && display !== "block") {
        inline = true;
        continue;
      }

      total += env.sizeOf(child, "offsetHeight");
    }

    if (inline && strutHeight) total += strutHeight;

    return total;
  };

  env.layoutMisses = function () { return layoutMisses; };

  if (viewportFacts && viewportFacts.view) {
    var view = viewportFacts.view;

    var WINDOW_METRICS = [
      "innerWidth", "innerHeight", "outerWidth", "outerHeight", "screenX", "screenY",
      "pageXOffset", "pageYOffset", "devicePixelRatio",
    ];

    for (var metric = 0; metric < WINDOW_METRICS.length; metric += 1) {
      (function (field) {
        if (typeof view[field] !== "number") return;
        patchGetter(globalThis, field, function () { return view[field]; });
      })(WINDOW_METRICS[metric]);
    }

    patchGetter(globalThis, "scrollX", function () { return view.pageXOffset || 0; });
    patchGetter(globalThis, "scrollY", function () { return view.pageYOffset || 0; });

    var visual = globalThis.visualViewport;

    if (visual) {
      var visualPrototype = protoOf(visual) || visual;
      if (typeof view.visualViewportWidth === "number") patchGetter(visualPrototype, "width", function () { return view.visualViewportWidth; });
      if (typeof view.visualViewportHeight === "number") patchGetter(visualPrototype, "height", function () { return view.visualViewportHeight; });
      if (typeof view.visualViewportScale === "number") patchGetter(visualPrototype, "scale", function () { return view.visualViewportScale; });
    }
  }

  var installMetrics = function (prototype, fields) {
    if (!prototype) return;

    for (var index = 0; index < fields.length; index += 1) {
      (function (field) {
        try {
          var previous = Object.getOwnPropertyDescriptor(prototype, field);

          Object.defineProperty(prototype, field, {
            get: asNative(function () {
              var state = nodeState(this);
              if (!state) return inherited(previous, this);
              return env.sizeOf(this, field);
            }, "get " + field),
            enumerable: previous ? previous.enumerable : true,
            configurable: true,
          });
        } catch (error) {}
      })(fields[index]);
    }
  };

  var installCounts = function (prototype) {
    if (!prototype) return;

    var fields = { childElementCount: "children" };

    for (var field in fields) {
      (function (name, source) {
        try {
          var previous = Object.getOwnPropertyDescriptor(prototype, name);

          Object.defineProperty(prototype, name, {
            get: asNative(function () {
              var state = nodeState(this);
              if (!state) return inherited(previous, this);
              var kids = state[source] || [];
              return kids.length;
            }, "get " + name),
            enumerable: previous ? previous.enumerable : true,
            configurable: true,
          });
        } catch (error) {}
      })(field, fields[field]);
    }
  };

  var installSizes = function (prototype) {
    if (!prototype) return;

    for (var side = 0; side < 2; side += 1) {
      (function (field) {
        try {
          var previous = Object.getOwnPropertyDescriptor(prototype, field);

          Object.defineProperty(prototype, field, {
            get: asNative(function () {
              var state = nodeState(this);
              if (state) return state[field];
              return inherited(previous, this);
            }, "get " + field),
            set: asNative(function (value) {
              var state = nodeState(this);
              if (!state) return;
              state[field] = Number(value) || 0;
              if (env.recordGraphics) env.recordGraphics(state.handle, "size", [field, state[field]]);
            }, "set " + field),
            enumerable: true,
            configurable: true,
          });
        } catch (error) {}
      })(side === 0 ? "width" : "height");
    }
  };

  var childFrames = [];

  var syncFrames = function () {
    for (var slot = 0; slot < 64; slot += 1) {
      try {
        if (Object.getOwnPropertyDescriptor(globalThis, String(slot))) delete globalThis[String(slot)];
      } catch (error) {}
    }

    for (var at = 0; at < childFrames.length; at += 1) {
      try {
        Object.defineProperty(globalThis, String(at), {
          value: childFrames[at].view,
          writable: false,
          enumerable: true,
          configurable: true,
        });
      } catch (error) {}
    }
  };

  try {
    Object.defineProperty(globalThis, "length", {
      get: asNative(function () { return childFrames.length; }, "get length"),
      enumerable: true,
      configurable: true,
    });
  } catch (error) {}

  var connected = function (node) {
    var current = node;

    for (var step = 0; current && step < 64; step += 1) {
      if (current === globalThis.document || (env.tree && (current === env.tree.body || current === env.tree.html || current === env.tree.head))) return true;
      current = current.parentNode;
    }

    return false;
  };

  var navigateFrame = function (element, view) {
    var state = nodeState(element);
    if (!state || !view) return;

    var src = state.attrs.src || state.src || "";
    if (!src || /^(about:|javascript:|data:)/.test(String(src))) return;

    var absolute;

    try {
      absolute = new globalThis.URL(String(src), globalThis.location.href).href;
    } catch (error) {
      return;
    }

    if (state.navigated === absolute) return;
    state.navigated = absolute;

    bridge.request("GET", absolute, JSON.stringify({ accept: "text/html,application/xhtml+xml,application/xml;q=0.9,*/*;q=0.8" }), null, function (status, headerJson, text) {
      if (!text || typeof bridge.runInFrame !== "function") return;

      record("frameLoad", { url: absolute, status: status, bytes: text.length });

      var pattern = /<script([^>]*)>([\s\S]*?)<\/script>/gi;
      var found;

      while ((found = pattern.exec(text))) {
        var attributes = found[1] || "";
        var inlineSource = found[2] || "";
        var srcMatch = /\bsrc\s*=\s*["']([^"']+)["']/i.exec(attributes);

        if (!srcMatch) {
          if (inlineSource.trim()) bridge.runInFrame(view, inlineSource, absolute, true);
          continue;
        }

        (function (address) {
          var target;

          try {
            target = new globalThis.URL(address.replace(/&amp;/g, "&"), absolute).href;
          } catch (error) {
            return;
          }

          bridge.request("GET", target, JSON.stringify({ accept: "*/*", referer: absolute }), null, function (childStatus, childHeaders, childText) {
            if (childText) bridge.runInFrame(view, childText, target, false);
          });
        })(srcMatch[1]);
      }
    });
  };

  var attachFrame = function (element) {
    var state = nodeState(element);
    if (!state || state.tag !== "iframe" || state.frame) return;

    var view = bridge.createFrame(state.attrs.src || state.src || "", element);
    if (!view) return;

    state.frame = view;
    childFrames.push({ element: element, view: view });
    syncFrames();
    navigateFrame(element, view);
  };

  env.navigateFrame = navigateFrame;

  var detachFrame = function (element) {
    var state = nodeState(element);

    for (var at = 0; at < childFrames.length; at += 1) {
      if (childFrames[at].element !== element) continue;

      childFrames.splice(at, 1);
      if (state) state.frame = undefined;
      syncFrames();
      return;
    }
  };

  var walkFrames = function (node, visit, depth) {
    if (!node || typeof node !== "object" || (depth || 0) > 16) return;

    var state = nodeState(node);
    if (state && state.tag === "iframe") visit(node);

    var children = state ? state.childNodes : node.childNodes;
    if (!Array.isArray(children)) return;

    for (var at = 0; at < children.length; at += 1) walkFrames(children[at], visit, (depth || 0) + 1);
  };

  var installFrame = function (prototype) {
    if (!prototype) return;

    try {
      Object.defineProperty(prototype, "contentWindow", {
        get: asNative(function () {
          var state = nodeState(this);
          if (!state) return null;

          if (state.frame === undefined && connected(this)) attachFrame(this);

          return state.frame === undefined ? null : state.frame;
        }, "get contentWindow"),
        enumerable: true,
        configurable: true,
      });

      Object.defineProperty(prototype, "contentDocument", {
        get: asNative(function () {
          var frame = this.contentWindow;
          return frame ? frame.document : null;
        }, "get contentDocument"),
        enumerable: true,
        configurable: true,
      });
    } catch (error) {}
  };

  var RAW_TEXT_TAGS = { script: 1, style: 1, xmp: 1, iframe: 1, noembed: 1, noframes: 1, noscript: 1, plaintext: 1 };

  var escapeText = function (value) {
    return String(value).replace(/&/g, "&amp;").replace(/ /g, "&nbsp;").replace(/</g, "&lt;").replace(/>/g, "&gt;");
  };

  var escapeAttribute = function (value) {
    return String(value).replace(/&/g, "&amp;").replace(/ /g, "&nbsp;").replace(/"/g, "&quot;");
  };

  var serializeNode = function (node, depth, raw) {
    if (!node || depth > 32) return "";

    if (node.nodeType === 3) return raw ? String(node.textContent) : escapeText(node.textContent);
    if (node.nodeType === 8) return "<!--" + String(node.textContent === undefined ? "" : node.textContent) + "-->";

    var state = nodeState(node);
    if (!state) return "";

    var tag = state.tag;
    var out = "<" + tag;
    var names = Object.keys(state.attrs || {});

    for (var index = 0; index < names.length; index += 1) {
      out += " " + names[index] + '="' + escapeAttribute(state.attrs[names[index]]) + '"';
    }

    out += ">";
    if (VOID_TAGS[tag]) return out;

    return out + serializeChildren(node, depth, Boolean(RAW_TEXT_TAGS[tag])) + "</" + tag + ">";
  };

  var serializeChildren = function (node, depth, raw) {
    var state = nodeState(node);
    var children = state ? state.childNodes : node.childNodes;
    if (!Array.isArray(children)) return "";

    var out = "";
    for (var index = 0; index < children.length; index += 1) out += serializeNode(children[index], depth + 1, raw);
    return out;
  };

  env.serializeNode = serializeNode;
  env.serializeChildren = serializeChildren;

  var installMarkup = function (prototype) {
    if (!prototype) return;

    try {
      Object.defineProperty(prototype, "outerHTML", {
        get: asNative(function () {
          return serializeNode(this, 0, false);
        }, "get outerHTML"),
        set: asNative(function () {}, "set outerHTML"),
        enumerable: true,
        configurable: true,
      });
    } catch (error) {}

    try {
      Object.defineProperty(prototype, "innerHTML", {
        get: asNative(function () {
          var state = nodeState(this);
          if (!state) return "";
          if (state.childNodes.length) return serializeChildren(this, 0, Boolean(RAW_TEXT_TAGS[state.tag]));
          return state.html || "";
        }, "get innerHTML"),
        set: asNative(function (value) {
          var state = nodeState(this);
          if (!state) return;
          state.childNodes.length = 0;
          state.children.length = 0;
          state.html = String(value);
          env.parseHtml(value, this);
        }, "set innerHTML"),
        enumerable: true,
        configurable: true,
      });
    } catch (error) {}
  };

  var sharedFieldsInstalled = false;
  var perTagInstalled = [];

  var VALUE_TAGS = ["input", "textarea", "select", "option", "button", "output", "progress", "meter", "param", "data", "li"];

  var REFLECTED = {
    input: ["type", "name", "size#20", "placeholder", "min", "max", "step", "pattern", "accept", "alt", "src", "width#0", "height#0", "maxLength:maxlength", "minLength:minlength", "autocomplete", "list", "multiple?", "disabled?", "required?", "readOnly?readonly"],
    textarea: ["name", "rows#2", "cols#20", "placeholder", "maxLength:maxlength", "disabled?", "readOnly?readonly", "required?"],
    select: ["name", "size", "multiple?", "disabled?", "required?"],
    button: ["type", "name", "disabled?"],
    img: ["src", "alt", "width#0", "height#0", "loading", "srcset", "sizes", "crossOrigin:crossorigin", "referrerPolicy:referrerpolicy"],
    video: ["src", "width#0", "height#0", "poster", "preload", "controls?", "loop?", "muted?", "autoplay?", "playsInline?playsinline"],
    audio: ["src", "preload", "controls?", "loop?", "muted?", "autoplay?"],
    iframe: ["src", "srcdoc", "name", "width", "height", "sandbox", "allow", "loading", "referrerPolicy:referrerpolicy"],
    a: ["href", "target", "rel", "download", "ping", "hreflang", "type"],
    script: ["src", "type", "integrity", "crossOrigin:crossorigin", "referrerPolicy:referrerpolicy", "async?", "defer?", "noModule?nomodule"],
    link: ["href", "rel", "media", "type", "as", "integrity", "crossOrigin:crossorigin"],
    form: ["name", "action", "method", "target", "enctype", "autocomplete", "noValidate?novalidate"],
    option: ["label", "disabled?"],
    canvas: [],
  };

  var installEntries = function (prototype) {
    var shape = traits.webkitEntries;
    if (!prototype || !shape || shape.type !== "object") return;

    try {
      Object.defineProperty(prototype, "webkitEntries", {
        get: asNative(function () { return []; }, "get webkitEntries"),
        set: undefined,
        enumerable: shape.enumerable !== false,
        configurable: shape.configurable !== false,
      });
    } catch (error) {}
  };

  var installReflected = function (prototype, tag) {
    var fields = REFLECTED[tag];
    if (!prototype || !fields) return;

    for (var index = 0; index < fields.length; index += 1) {
      (function (spec) {
        var boolean = spec.indexOf("?") !== -1;
        var numeric = spec.indexOf("#") !== -1;
        var fallback = numeric ? Number(spec.slice(spec.indexOf("#") + 1)) : 0;
        var parts = (numeric ? spec.slice(0, spec.indexOf("#")) : spec).replace("?", ":").split(":");
        var property = parts[0];
        var attribute = (parts[1] || property).toLowerCase();

        try {
          var previous = Object.getOwnPropertyDescriptor(prototype, property);

          Object.defineProperty(prototype, property, {
            get: asNative(function () {
              var state = nodeState(this);
              if (!state) return boolean ? false : "";
              var raw = state.attrs[attribute];
              if (boolean) return raw !== undefined;
              if (numeric) {
                if (raw === undefined) return fallback;
                var parsed = parseInt(raw, 10);
                return Number.isFinite(parsed) && parsed >= 0 ? parsed : fallback;
              }
              return raw === undefined ? "" : raw;
            }, "get " + property),
            set: asNative(function (value) {
              var state = nodeState(this);
              if (!state) return;

              if (boolean) {
                if (value) state.attrs[attribute] = "";
                else delete state.attrs[attribute];
                return;
              }

              state.attrs[attribute] = String(value);
            }, "set " + property),
            enumerable: previous ? previous.enumerable : true,
            configurable: true,
          });
        } catch (error) {}
      })(fields[index]);
    }
  };

  var installElementFields = function (tagPrototype, name) {
    if (!sharedFieldsInstalled) {
      sharedFieldsInstalled = true;

      var nodeProto = globalThis.Node && globalThis.Node.prototype;
      var elementProto = globalThis.Element && globalThis.Element.prototype;
      var htmlProto = globalThis.HTMLElement && globalThis.HTMLElement.prototype;

      installFields(nodeProto || tagPrototype, NODE_FIELDS);
      installFields(elementProto || tagPrototype, ELEMENT_FIELDS);
      installFields(htmlProto || tagPrototype, HTML_FIELDS);
      installCounts(elementProto || tagPrototype);
      installCounts(globalThis.Document && globalThis.Document.prototype);
      installMetrics(elementProto || tagPrototype, ["clientWidth", "clientHeight", "scrollWidth", "scrollHeight"]);
      installMetrics(htmlProto || tagPrototype, ["offsetWidth", "offsetHeight", "offsetLeft", "offsetTop"]);
      installMarkup(elementProto || tagPrototype);
    }

    if (!tagPrototype || perTagInstalled.indexOf(tagPrototype) !== -1) return;
    perTagInstalled.push(tagPrototype);

    installReflected(tagPrototype, name);

    if (VALUE_TAGS.indexOf(name) !== -1) installFields(tagPrototype, ["value"]);

    if (name === "canvas") installSizes(tagPrototype);
    if (name === "iframe") installFrame(tagPrototype);
    if (name === "input") installEntries(tagPrototype);
  };

  var contextHandles = new WeakMap();

  var handleOf = function (node) {
    var state = nodeState(node);
    return state ? state.handle : "canvas";
  };

  var attributesOf = function (node) {
    var state = nodeState(node);
    return state ? state.attrs : {};
  };

  var syncAttributes = function (node) {
    var state = nodeState(node);
    if (!state) return;

    var names = Object.keys(state.attrs);
    var list = state.attributes;

    if (!Array.isArray(list)) return;

    list.length = 0;

    for (var index = 0; index < names.length; index += 1) {
      list.push({ name: names[index], value: state.attrs[names[index]], localName: names[index], namespaceURI: null, specified: true });
    }
  };

  env.syncAttributes = syncAttributes;

  var hasAttributeNamed = function (node, key) {
    return Object.prototype.hasOwnProperty.call(attributesOf(node), key);
  };

  var makeElement = function (tag) {
    var name = String(tag).toLowerCase();
    var prototype = prototypeFor(name);
    var element = Object.create(prototype);

    elementCount += 1;
    installElementFields(prototype, name);

    var state = {
      tag: name,
      handle: name === "canvas" ? "canvas" + (canvasCount += 1) : "element" + elementCount,
      html: "",
      frame: undefined,
      attrs: {},
      tagName: name.toUpperCase(),
      nodeName: name.toUpperCase(),
      localName: name,
      nodeType: 1,
      children: [],
      childNodes: [],
      childElementCount: 0,
      firstElementChild: null,
      lastElementChild: null,
      previousElementSibling: null,
      nextElementSibling: null,
      attributes: [],
      style: null,
      parentNode: null,
      parentElement: null,
      firstChild: null,
      lastChild: null,
      nextSibling: null,
      previousSibling: null,
      ownerDocument: document,
      textContent: "",
      value: "",
      outerHTML: "",
      innerText: "",
      className: "",
      id: "",
      src: "",
      width: name === "canvas" ? 300 : 0,
      height: name === "canvas" ? 150 : 0,
      offsetWidth: 0,
      offsetHeight: 0,
      clientWidth: 0,
      clientHeight: 0,
    };

    state.style = makeStyle(element);
    elementState.set(element, state);
    styleOwners.set(state.style, element);

    if (!env.debugElements) return element;

    return new Proxy(element, {
      get: function (target, key) {
        var value = target[key];
        if (value === undefined && typeof key === "string") env.count("miss " + name + "." + key);
        return value;
      },
    });
  };

  env.makeElement = makeElement;

  var elementConstructor = function (name, tag, initialise) {
    var previous = globalThis[name];
    if (typeof previous !== "function") return;

    var made = function () {
      if (new.target === undefined) {
        throw env.hideFrames(new TypeError("Failed to construct '" + name + "': Please use the 'new' operator, this DOM object constructor cannot be called as a function."));
      }

      var element = makeElement(tag);
      initialise(element, arguments);
      return element;
    };

    asNative(made, name);

    try {
      Object.defineProperty(made, "length", { value: 0, configurable: true });
      Object.defineProperty(made, "prototype", { value: previous.prototype, writable: false, enumerable: false, configurable: false });
    } catch (error) {}

    try {
      Object.defineProperty(globalThis, name, { value: made, writable: true, enumerable: false, configurable: true });
    } catch (error) {}
  };

  elementConstructor("Audio", "audio", function (element, args) {
    element.preload = "auto";
    if (args.length) element.src = String(args[0]);
  });

  elementConstructor("Image", "img", function (element, args) {
    if (args.length > 0) element.width = Number(args[0]) || 0;
    if (args.length > 1) element.height = Number(args[1]) || 0;
  });

  elementConstructor("Option", "option", function (element, args) {
    if (args.length > 0) element.textContent = String(args[0]);
    if (args.length > 1) element.value = String(args[1]);
  });

  var srcdocValues = new WeakMap();
  var iframePrototype = prototypeFor("iframe");

  if (iframePrototype) {
    try {
      Object.defineProperty(iframePrototype, "srcdoc", {
        get: asNative(function () {
          return srcdocValues.has(this) ? srcdocValues.get(this) : "";
        }, "get srcdoc"),
        set: asNative(function (value) {
          srcdocValues.set(this, String(value));
        }, "set srcdoc"),
        enumerable: true,
        configurable: true,
      });
    } catch (error) {}
  }

  var listOf = function (items, brand) {
    var holder = globalThis[brand] && globalThis[brand].prototype ? Object.create(globalThis[brand].prototype) : [];

    if (holder === items) return items;

    for (var index = 0; index < items.length; index += 1) {
      try {
        Object.defineProperty(holder, index, { value: items[index], enumerable: true, configurable: true });
      } catch (error) {}
    }

    try {
      Object.defineProperty(holder, "length", { value: items.length, enumerable: false, configurable: true });
    } catch (error) {}

    if (brand === "NodeList") {
      try {
        Object.defineProperty(holder, Symbol.iterator, { value: Array.prototype[Symbol.iterator], enumerable: false, configurable: true, writable: true });
      } catch (error) {}
    }

    return holder;
  };

  var matchingElements = function (root, selector) {
    var wanted = String(selector).toLowerCase().trim();
    var everything = wanted === "*";
    var found = [];
    var roots = root === null || root === undefined
      ? (env.tree ? [env.tree.html, env.tree.head, env.tree.body] : [])
      : [root];

    var seen = new Set();

    var walk = function (node, depth) {
      if (!node || typeof node !== "object" || depth > 12 || seen.has(node) || found.length > 500) return;
      seen.add(node);

      var state = nodeState(node);
      var tag = state ? state.tag : String(node.tagName || "").toLowerCase();
      if (tag && (everything || tag === wanted)) found.push(node);

      var children = node.childNodes;
      if (!Array.isArray(children) && state) children = state.childNodes;
      if (!Array.isArray(children)) return;

      for (var index = 0; index < children.length; index += 1) walk(children[index], depth + 1);
    };

    if (!everything && !/^[a-z][a-z0-9-]*$/.test(wanted)) return found;

    for (var index = 0; index < roots.length; index += 1) walk(roots[index], 0);
    return found;
  };

  env.matchingElements = matchingElements;

  var runningScript = null;

  env.beginScript = function (url, inline) {
    if (!inline && (!url || !/^https?:/.test(String(url)))) return null;

    var element = makeElement("script");

    if (!inline) {
      element.src = String(url);
      element.async = false;
    }

    if (env.tree && env.tree.body) env.tree.body.appendChild(element);

    runningScript = element;
    return element;
  };

  env.endScript = function () {
    runningScript = null;
  };

  if (Document && Document.prototype) {
    patch(Document.prototype, "createElement", function createElement(tag) { return makeElement(tag); });
    patch(Document.prototype, "createElementNS", function createElementNS(ns, tag) { return makeElement(tag); });
    patch(Document.prototype, "createTextNode", function createTextNode(text) {
      var node = globalThis.Text && globalThis.Text.prototype ? Object.create(globalThis.Text.prototype) : {};
      var value = String(text);

      elementState.set(node, {
        tag: "#text",
        handle: "text",
        attrs: {},
        nodeType: 3,
        nodeName: "#text",
        data: value,
        textContent: value,
        wholeText: value,
        length: value.length,
        parentNode: null,
        parentElement: null,
        childNodes: [],
        children: [],
        ownerDocument: globalThis.document,
      });

      env.overrides.set(node, { data: value, wholeText: value, length: value.length });

      return node;
    });
    patch(Document.prototype, "createDocumentFragment", function createDocumentFragment() {
      var fragment = globalThis.DocumentFragment && globalThis.DocumentFragment.prototype
        ? Object.create(globalThis.DocumentFragment.prototype)
        : {};

      var children = [];

      elementState.set(fragment, {
        tag: "#document-fragment",
        handle: "fragment",
        attrs: {},
        nodeType: 11,
        nodeName: "#document-fragment",
        childNodes: children,
        children: children,
        childElementCount: 0,
        parentNode: null,
        parentElement: null,
        textContent: "",
        ownerDocument: globalThis.document,
      });

      env.overrides.set(fragment, { childElementCount: 0 });

      try {
        Object.defineProperty(fragment, "appendChild", {
          value: asNative(function appendChild(child) {
            children.push(child);
            return child;
          }, "appendChild"),
          writable: true,
          enumerable: false,
          configurable: true,
        });
      } catch (error) {}

      return fragment;
    });
    patch(Document.prototype, "getElementById", function getElementById() { return null; });
    patch(Document.prototype, "getElementsByTagName", function getElementsByTagName(tag) {
      return listOf(matchingElements(null, tag), "HTMLCollection");
    });
    patch(Document.prototype, "getElementsByClassName", function getElementsByClassName() { return listOf([], "HTMLCollection"); });
    patch(Document.prototype, "querySelector", function querySelector(selector) {
      var found = matchingElements(null, selector);
      return found.length ? found[0] : null;
    });
    patch(Document.prototype, "querySelectorAll", function querySelectorAll(selector) {
      return listOf(matchingElements(null, selector), "NodeList");
    });
    var refusedEvents = { touchevent: true, gamepadevent: true, devicemotionevent: true, deviceorientationevent: true };

    patch(Document.prototype, "createEvent", function createEvent(type) {
      var name = String(type).toLowerCase();

      if (refusedEvents[name]) {
        throw env.hideFrames(new DOMException("The provided event type ('" + type + "') is invalid.", "NotSupportedError"));
      }

      var constructors = {
        event: "Event",
        events: "Event",
        htmlevents: "Event",
        customevent: "CustomEvent",
        mouseevent: "MouseEvent",
        mouseevents: "MouseEvent",
        uievent: "UIEvent",
        uievents: "UIEvent",
        keyboardevent: "KeyboardEvent",
        messageevent: "MessageEvent",
        focusevent: "FocusEvent",
        wheelevent: "WheelEvent",
        pointerevent: "PointerEvent",
        progressevent: "ProgressEvent",
        storageevent: "StorageEvent",
      };

      var wanted = constructors[name];

      if (wanted && typeof globalThis[wanted] === "function") {
        try {
          return new globalThis[wanted]("");
        } catch (error) {}
      }

      return makeEvent(name);
    });
    patch(Document.prototype, "hasFocus", function hasFocus() { return false; });
    patch(Document.prototype, "createTreeWalker", function createTreeWalker() {
      return { nextNode: function () { return null; }, currentNode: null };
    });
    patch(Document.prototype, "evaluate", function evaluate() {
      return { iterateNext: function () { return null; }, snapshotLength: 0, snapshotItem: function () { return null; } };
    });
  }

  var VOID_TAGS = { area: 1, base: 1, br: 1, col: 1, embed: 1, hr: 1, img: 1, input: 1, link: 1, meta: 1, param: 1, source: 1, track: 1, wbr: 1 };
  var TAG = /<!--[\s\S]*?-->|<(\/?)([a-zA-Z][\w:-]*)((?:\s+[^\s=/>]+(?:\s*=\s*(?:"[^"]*"|'[^']*'|[^\s>]+))?)*)\s*(\/?)>|([^<]+)/g;

  var parseAttributes = function (text) {
    var out = {};
    var pattern = /([^\s=/>]+)(?:\s*=\s*(?:"([^"]*)"|'([^']*)'|([^\s>]+)))?/g;
    var match;

    while ((match = pattern.exec(text))) {
      out[match[1].toLowerCase()] = match[2] !== undefined ? match[2] : match[3] !== undefined ? match[3] : match[4] !== undefined ? match[4] : "";
    }

    return out;
  };

  var parseHtml = function (source, host) {
    var stack = [host];
    var match;

    TAG.lastIndex = 0;

    while ((match = TAG.exec(String(source)))) {
      var current = stack[stack.length - 1];

      if (match[5] !== undefined) {
        var text = match[5];

        if (text.trim()) {
          var node = { nodeType: 3, nodeName: "#text", textContent: text, parentNode: current };
          current.childNodes.push(node);
        }

        continue;
      }

      if (match[2] === undefined) continue;

      var tag = match[2].toLowerCase();

      if (match[1] === "/") {
        if (stack.length > 1 && nodeState(stack[stack.length - 1]) && nodeState(stack[stack.length - 1]).tag === tag) stack.pop();
        continue;
      }

      var element = makeElement(tag);
      var attributes = parseAttributes(match[3] || "");
      var names = Object.keys(attributes);

      for (var index = 0; index < names.length; index += 1) {
        try {
          element.setAttribute(names[index], attributes[names[index]]);
        } catch (error) {}
      }

      element.parentNode = current;
      current.childNodes.push(element);
      current.children.push(element);

      if (!VOID_TAGS[tag] && !match[4]) stack.push(element);
    }

    if (host.childNodes.length) {
      host.firstChild = host.childNodes[0];
      host.lastChild = host.childNodes[host.childNodes.length - 1];
    }

    return host;
  };

  env.parseHtml = parseHtml;

  var html = makeElement("html");
  var head = makeElement("head");
  var body = makeElement("body");

  var setOwn = function (target, key, value) {
    try {
      Object.defineProperty(target, key, { value: value, writable: true, enumerable: false, configurable: true });
    } catch (error) {}
  };

  html.children = [head, body];
  html.childNodes = [head, body];
  setOwn(head, "parentNode", html);
  setOwn(body, "parentNode", html);
  setOwn(html, "parentNode", document);

  env.tree = { html: html, head: head, body: body };

  var cookie = "";

  var parseUrl = function (href) {
    var match = /^(https?:)\/\/([^/?#:]+)(?::(\d+))?([^?#]*)(\?[^#]*)?(#.*)?$/.exec(String(href));

    if (!match) {
      return { href: String(href), protocol: "about:", host: "", hostname: "", port: "", pathname: "blank", search: "", hash: "", origin: "null" };
    }

    return {
      href: String(href),
      protocol: match[1],
      host: match[2] + (match[3] ? ":" + match[3] : ""),
      hostname: match[2],
      port: match[3] || "",
      pathname: match[4] || "/",
      search: match[5] || "",
      hash: match[6] || "",
      origin: match[1] + "//" + match[2] + (match[3] ? ":" + match[3] : ""),
    };
  };

  var pageUrl = bridge.pageUrl();
  var referrer = bridge.referrer();
  var parsedUrl = parseUrl(pageUrl);

  if (globalThis.location) {
    var locationKeys = Object.keys(parsedUrl);

    for (var k = 0; k < locationKeys.length; k += 1) {
      (function (key) {
        try {
          Object.defineProperty(globalThis.location, key, {
            get: asNative(function () { return parsedUrl[key]; }, "get " + key),
            set: asNative(function () {}, "set " + key),
            enumerable: true,
            configurable: true,
          });
        } catch (error) {}
      })(locationKeys[k]);
    }

    try {
      var ancestors = [];
      if (referrer) ancestors.push(parseUrl(referrer).origin);
      ancestors.item = asNative(function item(index) { return ancestors[index] === undefined ? null : ancestors[index]; }, "item");
      Object.defineProperty(ancestors, "length", { value: ancestors.length, configurable: true });

      Object.defineProperty(globalThis.location, "ancestorOrigins", {
        get: asNative(function () { return ancestors; }, "get ancestorOrigins"),
        enumerable: true,
        configurable: true,
      });
    } catch (error) {}

    patch(globalThis.location, "toString", function toString() { return parsedUrl.href; });
    patch(globalThis.location, "reload", function reload() { return undefined; });
    patch(globalThis.location, "assign", function assign() { return undefined; });
    patch(globalThis.location, "replace", function replace() { return undefined; });
  }

  if (Document && Document.prototype) {
    var documentValues = {
      documentElement: html,
      body: body,
      head: head,
      children: [html],
      childNodes: [html],
      firstChild: html,
      lastChild: html,
      all: [html, head, body],
      forms: [],
      links: [],
      images: [],
      scripts: [],
      embeds: [],
      plugins: [],
      styleSheets: [],
      anchors: [],
      readyState: "complete",
      hidden: false,
      visibilityState: "visible",
      title: "",
      referrer: referrer,
      URL: parsedUrl.href,
      documentURI: parsedUrl.href,
      baseURI: parsedUrl.href,
      domain: parsedUrl.hostname,
      characterSet: "UTF-8",
      charset: "UTF-8",
      contentType: "text/html",
      compatMode: "CSS1Compat",
      activeElement: body,
      currentScript: null,
      defaultView: globalThis,
      location: globalThis.location,
      nodeType: 9,
      nodeName: "#document",
    };

    Object.defineProperty(documentValues, "currentScript", {
      get: function () { return runningScript; },
      enumerable: true,
      configurable: true,
    });

    Object.defineProperty(documentValues, "scripts", {
      get: function () { return matchingElements(null, "script"); },
      enumerable: true,
      configurable: true,
    });

    var documentKeys = Object.keys(documentValues);

    for (var d = 0; d < documentKeys.length; d += 1) {
      (function (key) {
        try {
          delete document[key];
        } catch (error) {}

        patchGetter(Document.prototype, key, function () { return documentValues[key]; });
      })(documentKeys[d]);
    }

    try {
      env.cookies = function () { return cookie; };

      Object.defineProperty(Document.prototype, "cookie", {
        get: asNative(function () { return cookie; }, "get cookie"),
        set: asNative(function (value) {
          var pair = String(value).split(";")[0];
          cookie = cookie ? cookie + "; " + pair : pair;
        }, "set cookie"),
        enumerable: true,
        configurable: true,
      });
    } catch (error) {}
  }

  var relink = function (parent) {
    var state = nodeState(parent);
    if (!state || !Array.isArray(state.childNodes)) return;

    var kids = state.childNodes;
    state.firstChild = kids.length ? kids[0] : null;
    state.lastChild = kids.length ? kids[kids.length - 1] : null;

    var elements = [];

    for (var index = 0; index < kids.length; index += 1) {
      var childState = nodeState(kids[index]);
      if (childState && childState.nodeType === 1) elements.push(kids[index]);

      if (childState) {
        childState.previousSibling = index > 0 ? kids[index - 1] : null;
        childState.nextSibling = index + 1 < kids.length ? kids[index + 1] : null;
      }
    }

    state.children = elements;
    state.childElementCount = elements.length;
    state.firstElementChild = elements.length ? elements[0] : null;
    state.lastElementChild = elements.length ? elements[elements.length - 1] : null;
  };

  var appendChild = function appendChild(child) {
    if (child && typeof child === "object") {
      child.parentNode = this;
      if (Array.isArray(this.childNodes)) this.childNodes.push(child);
      if (Array.isArray(this.children)) this.children.push(child);
      if (nodeState(child) && nodeState(child).tag === "script" && child.src) record("script", child.src);
      if (nodeState(child) && nodeState(child).tag === "iframe") record("iframe", child.src || "about:blank");

      var childState = nodeState(child);
      if (childState) childState.parentElement = nodeState(this) && nodeState(this).nodeType === 1 ? this : null;

      relink(this);
      if (connected(this)) walkFrames(child, attachFrame);
    }

    return child;
  };

  var detachChild = function (parent, child) {
    if (!child || typeof child !== "object") return child;

    if (parent && Array.isArray(parent.childNodes)) {
      var at = parent.childNodes.indexOf(child);
      if (at !== -1) parent.childNodes.splice(at, 1);
    }

    if (parent && Array.isArray(parent.children)) {
      var where = parent.children.indexOf(child);
      if (where !== -1) parent.children.splice(where, 1);
    }

    try {
      child.parentNode = null;
    } catch (error) {}

    var goneState = nodeState(child);
    if (goneState) {
      goneState.parentElement = null;
      goneState.previousSibling = null;
      goneState.nextSibling = null;
    }

    relink(parent);
    walkFrames(child, detachFrame);

    return child;
  };

  var nodePrototype = Node && Node.prototype ? Node.prototype : Element && Element.prototype;

  if (nodePrototype) {
    patch(nodePrototype, "appendChild", appendChild);
    patch(nodePrototype, "insertBefore", function insertBefore(child) { return appendChild.call(this, child); });
    patch(nodePrototype, "removeChild", function removeChild(child) {
      return detachChild(this, child);
    });
    patch(nodePrototype, "contains", function contains(other) {
      var current = other;

      for (var step = 0; current && step < 64; step += 1) {
        if (current === this) return true;
        current = current.parentNode;
      }

      return false;
    });

    patch(Element.prototype, "matches", function matches(selector) {
      var state = nodeState(this);
      if (!state) return false;

      var wanted = String(selector).trim().toLowerCase();

      if (/^[a-z][a-z0-9-]*$/.test(wanted)) return state.tag === wanted;
      if (wanted.charAt(0) === "#") return state.attrs.id === wanted.slice(1);
      if (wanted.charAt(0) === ".") return String(state.attrs.class || "").split(/\s+/).indexOf(wanted.slice(1)) !== -1;

      return false;
    });

    patch(Element.prototype, "closest", function closest(selector) {
      var current = this;

      for (var step = 0; current && step < 64; step += 1) {
        if (typeof current.matches === "function" && current.matches(selector)) return current;
        current = current.parentNode;
      }

      return null;
    });

    patch(nodePrototype, "dispatchEvent", function dispatchEvent(event) {
      dispatch(this, event);
      return true;
    });

    patch(nodePrototype, "getRootNode", function getRootNode() {
      var current = this;

      for (var step = 0; step < 64; step += 1) {
        var parent = current.parentNode;
        if (!parent) return current;
        current = parent;
      }

      return current;
    });
    patch(nodePrototype, "cloneNode", function cloneNode() { return makeElement((nodeState(this) || {}).tag || "div"); });
  }

  if (Element && Element.prototype) {
    patch(Element.prototype, "setAttribute", function setAttribute(key, value) {
      attributesOf(this)[key] = String(value);
      syncAttributes(this);

      var attrState = nodeState(this);

      if (attrState) {
        if (key === "class") attrState.className = String(value);
        if (key === "id") attrState.id = String(value);
        if (key === "src") attrState.src = String(value);
      }

      var attrState = nodeState(this);
      if (key === "src" && attrState && attrState.tag === "iframe" && attrState.frame) navigateFrame(this, attrState.frame);
    });
    patch(Element.prototype, "getAttribute", function getAttribute(key) {
      return hasAttributeNamed(this, key) ? attributesOf(this)[key] : null;
    });
    patch(Element.prototype, "hasAttribute", function hasAttribute(key) {
      return hasAttributeNamed(this, key);
    });
    patch(Element.prototype, "removeAttribute", function removeAttribute(key) {
      delete attributesOf(this)[key];
      syncAttributes(this);
    });
    var makeRect = function (box) {
      var rect = globalThis.DOMRect && globalThis.DOMRect.prototype ? Object.create(globalThis.DOMRect.prototype) : {};

      var fields = {
        x: box.x,
        y: box.y,
        width: box.width,
        height: box.height,
        top: box.y,
        right: box.x + box.width,
        bottom: box.y + box.height,
        left: box.x,
      };

      env.overrides.set(rect, fields);

      for (var name in fields) {
        try {
          Object.defineProperty(rect, name, { value: fields[name], writable: true, enumerable: true, configurable: true });
        } catch (error) {}
      }

      try {
        Object.defineProperty(rect, "toJSON", {
          value: asNative(function toJSON() { return fields; }, "toJSON"),
          writable: true,
          enumerable: false,
          configurable: true,
        });
      } catch (error) {}

      return rect;
    };

    env.makeRect = makeRect;

    patch(Element.prototype, "getBoundingClientRect", function getBoundingClientRect() {
      return makeRect(env.boxOf(this));
    });

    patch(Element.prototype, "getClientRects", function getClientRects() {
      var box = env.boxOf(this);
      if (!box.width && !box.height) return [];

      return [{
        x: box.x,
        y: box.y,
        width: box.width,
        height: box.height,
        top: box.y,
        right: box.x + box.width,
        bottom: box.y + box.height,
        left: box.x,
      }];
    });
    patch(Element.prototype, "querySelector", function querySelector() { return null; });
    patch(Element.prototype, "querySelectorAll", function querySelectorAll(selector) {
      return listOf(matchingElements(this, selector), "NodeList");
    });
    var shadowRoots = new WeakMap();

    patch(Element.prototype, "checkVisibility", function checkVisibility() { return true; });

    patch(Element.prototype, "attachShadow", function attachShadow(init) {
      var mode = init && init.mode ? String(init.mode) : "open";
      var root = globalThis.ShadowRoot ? Object.create(globalThis.ShadowRoot.prototype) : {};

      env.overrides.set(root, {
        mode: mode,
        host: this,
        delegatesFocus: Boolean(init && init.delegatesFocus),
        clonable: Boolean(init && init.clonable),
        serializable: Boolean(init && init.serializable),
        slotAssignment: init && init.slotAssignment ? String(init.slotAssignment) : "named",
        innerHTML: "",
        childNodes: [],
        children: [],
        nodeType: 11,
        nodeName: "#document-fragment",
        activeElement: null,
        adoptedStyleSheets: [],
        styleSheets: [],
      });

      shadowRoots.set(this, { mode: mode, root: root });
      return root;
    });

    patchGetter(Element.prototype, "shadowRoot", function () {
      var found = shadowRoots.get(this);
      return found && found.mode === "open" ? found.root : null;
    });
    patch(Element.prototype, "remove", function remove() {
      detachChild(this.parentNode, this);
      return undefined;
    });
  }

  if (HTMLElement && HTMLElement.prototype) {
    patch(HTMLElement.prototype, "focus", function focus() { return undefined; });
    patch(HTMLElement.prototype, "blur", function blur() { return undefined; });
    patch(HTMLElement.prototype, "click", function click() { return undefined; });
  }

  var gfxCalls = [];
  var gfxReplies = env.graphicsReplies || [];
  var gfxIndex = 0;
  env.gfxCalls = gfxCalls;

  var simple = function (value) {
    if (value === null || value === undefined) return null;
    var type = typeof value;
    if (type === "number" || type === "string" || type === "boolean") return value;

    if (value && typeof value.length === "number" && type === "object") {
      var out = [];
      for (var index = 0; index < Math.min(value.length, 64); index += 1) out.push(simple(value[index]));
      return { list: out, length: value.length, kind: value.constructor ? value.constructor.name : "Array" };
    }

    if (value && contextHandles.has(value)) return { handle: contextHandles.get(value) };

    if (value && typeof value.width === "number" && typeof value.height === "number" && value.data && typeof value.data.length === "number") {
      var pixels = [];
      for (var p = 0; p < value.data.length; p += 1) pixels.push(value.data[p]);
      return { imagedata: { width: value.width, height: value.height, data: pixels } };
    }
    if (value && value.__extension) return { extension: value.__extension };

    if (type === "object") {
      var plain = {};
      for (var key in value) {
        try {
          var member = value[key];
          if (member === null || ["number", "string", "boolean"].indexOf(typeof member) !== -1) plain[key] = member;
        } catch (error) {}
      }
      return plain;
    }

    return null;
  };

  var materialiseReply = function (reply) {
    if (!reply || reply.kind === "primitive") return reply ? reply.value : null;

    if (reply.kind === "imagedata") {
      var data = new Uint8ClampedArray(reply.data);
      var image = Object.create(globalThis.ImageData ? globalThis.ImageData.prototype : Object.prototype);
      Object.defineProperty(image, "data", { value: data, enumerable: true });
      Object.defineProperty(image, "width", { value: reply.width, enumerable: true });
      Object.defineProperty(image, "height", { value: reply.height, enumerable: true });
      return image;
    }

    if (reply.kind === "typed") {
      var Constructor = globalThis[reply.name] || Float32Array;
      return new Constructor(reply.values);
    }

    if (reply.kind === "array") return reply.values.slice();

    if (reply.kind === "extension") {
      var extension = {};
      var keys = Object.keys(reply.constants || {});
      for (var index = 0; index < keys.length; index += 1) extension[keys[index]] = reply.constants[keys[index]];
      extension.__extension = reply.id;
      return extension;
    }

    if (reply.kind === "object") {
      var object = {};
      var fields = Object.keys(reply.value || {});
      for (var f = 0; f < fields.length; f += 1) object[fields[f]] = reply.value[fields[f]];
      return object;
    }

    return null;
  };

  var replyIndex = {};
  var replyCursor = {};

  var keyFor = function (handle, method, args) {
    return handle + "|" + method + "|" + JSON.stringify(args);
  };

  for (var r = 0; r < gfxReplies.length; r += 1) {
    var stored = gfxReplies[r];
    var storedKey = keyFor(stored.handle, stored.method, stored.args);
    if (!replyIndex[storedKey]) replyIndex[storedKey] = [];
    replyIndex[storedKey].push(stored);
  }

  var webgl = env.webglProfiles || {};

  var profileFor = function (handle) {
    var kind = String(handle).split(":")[1];
    return webgl[kind] || null;
  };

  var fromProfile = function (handle, method, args) {
    var profile = profileFor(handle);
    if (!profile) return undefined;

    if (method === "getParameter") {
      var entry = profile.parameters[String(args[0])];
      return entry ? materialiseReply(entry) : undefined;
    }

    if (method === "getSupportedExtensions") return profile.supported.slice();

    if (method === "getExtension") {
      var constants = profile.extensions[String(args[0])];
      if (!constants) return null;
      var extension = {};
      var keys = Object.keys(constants);
      for (var index = 0; index < keys.length; index += 1) extension[keys[index]] = constants[keys[index]];
      return extension;
    }

    if (method === "getShaderPrecisionFormat") {
      var format = profile.precision[String(args[0]) + ":" + String(args[1])];
      return format ? { rangeMin: format.rangeMin, rangeMax: format.rangeMax, precision: format.precision } : undefined;
    }

    if (method === "getContextAttributes") {
      if (!profile.attributes) return undefined;
      var attributes = {};
      var fields = Object.keys(profile.attributes);
      for (var f = 0; f < fields.length; f += 1) attributes[fields[f]] = profile.attributes[fields[f]];
      return attributes;
    }

    return undefined;
  };

  env.makeMetrics = function (fields) {
    var metrics = globalThis.TextMetrics && globalThis.TextMetrics.prototype
      ? Object.create(globalThis.TextMetrics.prototype)
      : {};

    env.overrides.set(metrics, fields);

    for (var name in fields) {
      try {
        Object.defineProperty(metrics, name, { value: fields[name], writable: false, enumerable: true, configurable: true });
      } catch (error) {}
    }

    return metrics;
  };

  var shapeFor = function (method, args) {
    if (method === "measureText") {
      return env.makeMetrics({
        width: 0,
        actualBoundingBoxLeft: 0,
        actualBoundingBoxRight: 0,
        fontBoundingBoxAscent: 10,
        fontBoundingBoxDescent: 2,
        actualBoundingBoxAscent: 0,
        actualBoundingBoxDescent: 0,
        hangingBaseline: 8,
        alphabeticBaseline: 0,
        ideographicBaseline: -2,
      });
    }

    if (method === "getImageData") {
      var width = Number(args[2]) || 1;
      var height = Number(args[3]) || 1;
      return { width: width, height: height, data: new Uint8ClampedArray(width * height * 4), colorSpace: "srgb" };
    }

    if (method === "createImageData") {
      var w = Number(args[0]) || 1;
      var h = Number(args[1]) || 1;
      return { width: w, height: h, data: new Uint8ClampedArray(w * h * 4), colorSpace: "srgb" };
    }

    return undefined;
  };

  var recordGraphics = function (handle, method, args) {
    var encoded = [];
    for (var index = 0; index < args.length; index += 1) encoded.push(simple(args[index]));

    var entry = { handle: handle, method: method, args: encoded };
    gfxCalls.push(entry);
    gfxIndex += 1;

    var pure = fromProfile(handle, method, encoded);
    if (pure !== undefined) return pure;

    var key = keyFor(handle, method, encoded);
    var bucket = replyIndex[key];

    if (!bucket || !bucket.length) {
      if (gfxReplies.length) env.count("gfx miss " + method);
      return shapeFor(method, encoded);
    }

    var cursor = replyCursor[key] || 0;
    var reply = bucket[Math.min(cursor, bucket.length - 1)];
    replyCursor[key] = cursor + 1;

    return materialiseReply(reply.result);
  };

  env.graphicsCallCount = function () {
    return gfxCalls.length;
  };

  env.recordGraphics = recordGraphics;

  var wrapContextPrototype = function (constructor, label) {
    if (!constructor || !constructor.prototype) return;

    var names = Object.getOwnPropertyNames(constructor.prototype);

    for (var index = 0; index < names.length; index += 1) {
      var name = names[index];
      if (name === "constructor" || name === "canvas") continue;

      var descriptor = Object.getOwnPropertyDescriptor(constructor.prototype, name);
      if (!descriptor || typeof descriptor.value !== "function") continue;

      (function (method, arity) {
        try {
          patch(constructor.prototype, method, function () {
            return recordGraphics(contextHandles.get(this) || label, method, arguments);
          });

          Object.defineProperty(constructor.prototype[method], "length", { value: arity, configurable: true });
        } catch (error) {}
      })(name, descriptor.value.length);
    }
  };

  wrapContextPrototype(globalThis.CanvasRenderingContext2D, "2d");
  wrapContextPrototype(globalThis.WebGLRenderingContext, "webgl");
  wrapContextPrototype(globalThis.WebGL2RenderingContext, "webgl2");

  var contextFor = function (canvas, kind) {
    var normalised = String(kind).toLowerCase();

    var constructor =
      normalised === "2d" ? globalThis.CanvasRenderingContext2D :
      normalised === "webgl2" ? globalThis.WebGL2RenderingContext :
      normalised === "webgl" || normalised === "experimental-webgl" ? globalThis.WebGLRenderingContext :
      null;

    if (!constructor || !constructor.prototype) return null;

    var context = Object.create(constructor.prototype);
    contextHandles.set(context, handleOf(canvas) + ":" + normalised);
    Object.defineProperty(context, "canvas", { value: canvas, enumerable: true });
    return context;
  };

  if (globalThis.HTMLCanvasElement && globalThis.HTMLCanvasElement.prototype) {
    patch(globalThis.HTMLCanvasElement.prototype, "getContext", function getContext(kind, attributes) {
            var answer = recordGraphics(handleOf(this), "getContext", [String(kind), attributes]);
      if (gfxReplies.length && answer === null) return null;
      return contextFor(this, kind);
    });

    patch(globalThis.HTMLCanvasElement.prototype, "toDataURL", function toDataURL(type) {
      var value = recordGraphics(handleOf(this), "toDataURL", arguments);
      return typeof value === "string" ? value : "data:,";
    });

    patch(globalThis.HTMLCanvasElement.prototype, "toBlob", function toBlob(callback) {
      env.later(function () { callback(null); }, 0);
    });
  }

  if (globalThis.HTMLMediaElement && globalThis.HTMLMediaElement.prototype) {
    patch(globalThis.HTMLMediaElement.prototype, "canPlayType", function canPlayType(type) {
      var tag = (nodeState(this) || {}).tag || "media";
      var table = env.media ? env.media[tag === "audio" ? "audio" : "video"] : null;

      if (table && Object.prototype.hasOwnProperty.call(table, String(type))) return table[String(type)];

      var answer = env.recordGraphics ? env.recordGraphics(tag, "canPlayType", [String(type)]) : undefined;
      return typeof answer === "string" ? answer : bridge.canPlayType(String(type));
    });
    patch(globalThis.HTMLMediaElement.prototype, "play", function play() { return Promise.resolve(); });
    patch(globalThis.HTMLMediaElement.prototype, "load", function load() { return undefined; });
  }

  var storagePrototype = protoOf(globalThis.localStorage);

  if (storagePrototype) {
    var stores = new WeakMap();

    var stateOf = function (instance) {
      var state = stores.get(instance);

      if (!state) {
        state = Object.create(null);
        stores.set(instance, state);
      }

      return state;
    };

    patch(storagePrototype, "getItem", function getItem(key) {
      var state = stateOf(this);
      return Object.prototype.hasOwnProperty.call(state, String(key)) ? state[String(key)] : null;
    });
    patch(storagePrototype, "setItem", function setItem(key, value) {
      stateOf(this)[String(key)] = String(value);
    });
    patch(storagePrototype, "removeItem", function removeItem(key) {
      delete stateOf(this)[String(key)];
    });
    patch(storagePrototype, "clear", function clear() {
      stores.set(this, Object.create(null));
    });
    patch(storagePrototype, "key", function key(index) {
      return Object.keys(stateOf(this))[index] || null;
    });
    patchGetter(storagePrototype, "length", function () {
      return Object.keys(stateOf(this)).length;
    });
  }

  var cryptoPrototype = protoOf(globalThis.crypto);

  if (cryptoPrototype) {
    patch(cryptoPrototype, "getRandomValues", function getRandomValues(view) {
      var hex = bridge.random(view.byteLength);
      var out = new Uint8Array(view.buffer, view.byteOffset, view.byteLength);
      for (var index = 0; index < out.length; index += 1) out[index] = parseInt(hex.substr(index * 2, 2), 16);
      return view;
    });
    patch(cryptoPrototype, "randomUUID", function randomUUID() { return bridge.uuid(); });
  }

  if (globalThis.SubtleCrypto && globalThis.SubtleCrypto.prototype && bridge.digest) {
    var hexOf = function (data) {
      var bytes = null;
      var brand = Object.prototype.toString.call(data);

      if (brand === "[object ArrayBuffer]") bytes = new Uint8Array(data);
      else if (ArrayBuffer.isView(data)) bytes = new Uint8Array(data.buffer, data.byteOffset || 0, data.byteLength);
      else if (data && typeof data.length === "number" && typeof data[0] === "number") bytes = data;
      else if (typeof data === "string") bytes = utf8Encode(data);

      if (!bytes) return null;

      var out = "";
      for (var index = 0; index < bytes.length; index += 1) out += (bytes[index] < 16 ? "0" : "") + bytes[index].toString(16);
      return out;
    };

    patch(globalThis.SubtleCrypto.prototype, "digest", function digest(algorithm, data) {
      var name = algorithm && typeof algorithm === "object" ? algorithm.name : algorithm;
      var hex = hexOf(data);

      if (hex === null) {
        env.count("digest refused " + Object.prototype.toString.call(data));
        return Promise.reject(new TypeError("Failed to execute 'digest' on 'SubtleCrypto': The provided value is not of type '(ArrayBuffer or ArrayBufferView)'."));
      }

      var out = bridge.digest(String(name), hex);

      if (out === null) {
        var error = new Error("Unrecognized name.");
        error.name = "NotSupportedError";
        return Promise.reject(env.hideFrames(error));
      }

      var bytes = new Uint8Array(out.length / 2);
      for (var index = 0; index < bytes.length; index += 1) bytes[index] = parseInt(out.substr(index * 2, 2), 16);
      return Promise.resolve(bytes.buffer);
    });
  }

  var performancePrototype = protoOf(globalThis.performance);

  if (performancePrototype) {
    patch(performancePrototype, "now", function now() { return env.clock(); });
    patch(performancePrototype, "getEntriesByType", function getEntriesByType(type) { return JSON.parse(bridge.entries(String(type))); });
    patch(performancePrototype, "getEntriesByName", function getEntriesByName() { return []; });
    patch(performancePrototype, "getEntries", function getEntries() { return JSON.parse(bridge.entries("navigation")); });
    patch(performancePrototype, "mark", function mark() { return undefined; });
    patch(performancePrototype, "measure", function measure() { return undefined; });
    patch(performancePrototype, "clearMarks", function clearMarks() { return undefined; });
  }


  if (globalThis.XMLHttpRequest && globalThis.XMLHttpRequest.prototype) {
    var xhr = globalThis.XMLHttpRequest.prototype;

    patch(xhr, "open", function open(method, url) {
      this.__method = String(method);
      this.__url = String(url);
      this.__headers = {};
      this.readyState = 1;
    });

    patch(xhr, "setRequestHeader", function setRequestHeader(name, value) {
      if (!this.__headers) this.__headers = {};
      this.__headers[String(name).toLowerCase()] = String(value);
    });

    patch(xhr, "send", function send(body) {
      var request = this;
      var bytes = body ? body.byteLength || body.length || 0 : 0;
      record("xhr", { method: request.__method, url: request.__url, headers: request.__headers, bytes: bytes });

      bridge.request(request.__method, request.__url, JSON.stringify(request.__headers || {}), body, function (status, headerJson, text) {
        var headers = JSON.parse(headerJson);
        request.readyState = 4;
        request.status = status;
        request.responseText = text;
        request.response = text;

        patch(request, "getAllResponseHeaders", function getAllResponseHeaders() {
          return Object.keys(headers).map(function (key) { return key + ": " + headers[key]; }).join("\r\n");
        });

        patch(request, "getResponseHeader", function getResponseHeader(name) {
          var value = headers[String(name).toLowerCase()];
          return value === undefined ? null : value;
        });

        dispatch(request, makeEvent("readystatechange"));
        dispatch(request, makeEvent("load"));
        dispatch(request, makeEvent("loadend"));
      });
    });

    patch(xhr, "abort", function abort() { return undefined; });
    patch(xhr, "overrideMimeType", function overrideMimeType() { return undefined; });
    patch(xhr, "getAllResponseHeaders", function getAllResponseHeaders() { return ""; });
    patch(xhr, "getResponseHeader", function getResponseHeader() { return null; });
  }

  var navigatorPrototype = protoOf(globalThis.navigator);

  var batteryFacts = traits.battery || {};

  var recordedDuration = function (name) {
    return (traits.durations && traits.durations[name]) || 0;
  };

  if (navigatorPrototype) {
    patch(navigatorPrototype, "getBattery", function getBattery() {
      env.spend(recordedDuration("battery"));

      return Promise.resolve({
        charging: batteryFacts.charging !== undefined ? batteryFacts.charging : true,
        chargingTime: batteryFacts.chargingTime === "Infinity" || batteryFacts.chargingTime === undefined ? Infinity : batteryFacts.chargingTime,
        dischargingTime: batteryFacts.dischargingTime === "Infinity" || batteryFacts.dischargingTime === undefined ? Infinity : batteryFacts.dischargingTime,
        level: batteryFacts.level !== undefined ? batteryFacts.level : 0.8,
        onchargingchange: null,
        onchargingtimechange: null,
        ondischargingtimechange: null,
        onlevelchange: null,
        addEventListener: addListener,
        removeEventListener: removeListener,
      });
    });

    patch(navigatorPrototype, "sendBeacon", function sendBeacon() { return true; });
    patch(navigatorPrototype, "javaEnabled", function javaEnabled() { return false; });
    patch(navigatorPrototype, "vibrate", function vibrate() { return false; });
  }

  var keyboard = globalThis.navigator && globalThis.navigator.keyboard;

  if (keyboard && traits.keyboardLayout) {
    var layoutPairs = traits.keyboardLayout;

    var layoutMap = function () {
      var proto = globalThis.KeyboardLayoutMap && globalThis.KeyboardLayoutMap.prototype;
      var map = proto ? Object.create(proto) : {};
      var keys = [];
      var values = [];

      for (var index = 0; index < layoutPairs.length; index += 1) {
        keys.push(layoutPairs[index][0]);
        values.push(layoutPairs[index][1]);
      }

      var pairsOf = function () {
        var out = [];
        for (var at = 0; at < keys.length; at += 1) out.push([keys[at], values[at]]);
        return out;
      };

      var fields = {
        size: keys.length,
        get: asNative(function get(key) {
          var at = keys.indexOf(String(key));
          return at === -1 ? undefined : values[at];
        }, "get"),
        has: asNative(function has(key) { return keys.indexOf(String(key)) !== -1; }, "has"),
        keys: asNative(function keys_() { return keys.slice()[Symbol.iterator](); }, "keys"),
        values: asNative(function values_() { return values.slice()[Symbol.iterator](); }, "values"),
        entries: asNative(function entries() { return pairsOf()[Symbol.iterator](); }, "entries"),
        forEach: asNative(function forEach(callback, self) {
          for (var at = 0; at < keys.length; at += 1) callback.call(self, values[at], keys[at], map);
          return undefined;
        }, "forEach"),
      };

      for (var name in fields) {
        Object.defineProperty(map, name, { value: fields[name], writable: false, enumerable: false, configurable: true });
      }

      Object.defineProperty(map, Symbol.iterator, { value: fields.entries, writable: false, enumerable: false, configurable: true });
      return map;
    };

    patch(protoOf(keyboard) || keyboard, "getLayoutMap", function getLayoutMap() {
      var made = layoutMap();

      return new Promise(function (resolve) {
        later(function () {
          env.spend(recordedDuration("keyboardLayout"));
          resolve(made);
        }, 0);
      });
    });
  }

  var mediaDevices = globalThis.navigator && globalThis.navigator.mediaDevices;

  if (mediaDevices) {
    var deviceInfo = function (kind) {
      var proto = globalThis.InputDeviceInfo && kind === "videoinput" ? globalThis.InputDeviceInfo.prototype : globalThis.MediaDeviceInfo && globalThis.MediaDeviceInfo.prototype;
      var info = proto ? Object.create(proto) : {};

      env.overrides.set(info, { deviceId: "", kind: kind, label: "", groupId: "" });

      Object.defineProperty(info, "deviceId", { value: "", enumerable: true, configurable: true });
      Object.defineProperty(info, "kind", { value: kind, enumerable: true, configurable: true });
      Object.defineProperty(info, "label", { value: "", enumerable: true, configurable: true });
      Object.defineProperty(info, "groupId", { value: "", enumerable: true, configurable: true });

      return info;
    };

    patch(protoOf(mediaDevices) || mediaDevices, "enumerateDevices", function enumerateDevices() {
      return Promise.resolve([deviceInfo("audioinput"), deviceInfo("videoinput"), deviceInfo("audiooutput")]);
    });

    patch(protoOf(mediaDevices) || mediaDevices, "getSupportedConstraints", function getSupportedConstraints() {
      return {
        aspectRatio: true, autoGainControl: true, channelCount: true, deviceId: true, displaySurface: true,
        echoCancellation: true, facingMode: true, frameRate: true, groupId: true, height: true,
        noiseSuppression: true, sampleRate: true, sampleSize: true, width: true,
      };
    });
  }

  if (globalThis.navigator && globalThis.navigator.storage) {
    patch(protoOf(globalThis.navigator.storage) || globalThis.navigator.storage, "estimate", function estimate() {
      return Promise.resolve({ quota: 3221225472, usage: 0, usageDetails: {} });
    });

    patch(protoOf(globalThis.navigator.storage) || globalThis.navigator.storage, "persisted", function persisted() {
      return Promise.resolve(false);
    });
  }

  if (globalThis.FontFace && globalThis.FontFace.prototype) {
    patch(globalThis.FontFace.prototype, "load", function load() {
      var face = this;
      return Promise.resolve(face);
    });
  }

  for (var aiIndex = 0; aiIndex < 4; aiIndex += 1) {
    var aiName = ["LanguageDetector", "Translator", "Summarizer", "LanguageModel"][aiIndex];
    var aiClass = globalThis[aiName];

    if (!aiClass) continue;

    patch(aiClass, "availability", function availability() { return Promise.resolve("available"); });
    patch(aiClass, "create", function create() { return Promise.reject(new DOMException("Model not available", "NotSupportedError")); });
  }

  var audioParam = function (value) {
    return {
      value: value,
      defaultValue: value,
      minValue: -3.4028234663852886e38,
      maxValue: 3.4028234663852886e38,
      automationRate: "a-rate",
      setValueAtTime: asNative(function setValueAtTime() { return this; }, "setValueAtTime"),
      linearRampToValueAtTime: asNative(function linearRampToValueAtTime() { return this; }, "linearRampToValueAtTime"),
      exponentialRampToValueAtTime: asNative(function exponentialRampToValueAtTime() { return this; }, "exponentialRampToValueAtTime"),
      setTargetAtTime: asNative(function setTargetAtTime() { return this; }, "setTargetAtTime"),
      cancelScheduledValues: asNative(function cancelScheduledValues() { return this; }, "cancelScheduledValues"),
    };
  };

  var audioNode = function (kind, extra) {
    var node = {
      channelCount: 2,
      channelCountMode: kind === "analyser" ? "max" : "explicit",
      channelInterpretation: "speakers",
      numberOfInputs: 1,
      numberOfOutputs: 1,
      connect: asNative(function connect(target) { return target; }, "connect"),
      disconnect: asNative(function disconnect() { return undefined; }, "disconnect"),
      addEventListener: addListener,
      removeEventListener: removeListener,
    };

    if (extra) for (var key in extra) node[key] = extra[key];
    return node;
  };

  var audioBuffer = function (channels, length, rate, samples) {
    return {
      numberOfChannels: channels,
      length: length,
      sampleRate: rate,
      duration: length / rate,
      getChannelData: asNative(function getChannelData(index) {
        if (samples && (index === 0 || index === undefined)) return samples.slice(0, length);
        return new Float32Array(length);
      }, "getChannelData"),
      copyFromChannel: asNative(function copyFromChannel() { return undefined; }, "copyFromChannel"),
      copyToChannel: asNative(function copyToChannel() { return undefined; }, "copyToChannel"),
    };
  };

  var renderedSamples = null;

  var recordedChannel = function () {
    if (renderedSamples !== null) return renderedSamples;
    if (!traits.audio || !traits.audio.channel) return null;

    var binary = globalThis.atob(traits.audio.channel);
    var bytes = new Uint8Array(binary.length);

    for (var index = 0; index < binary.length; index += 1) bytes[index] = binary.charCodeAt(index);

    renderedSamples = new Float32Array(bytes.buffer, bytes.byteOffset, bytes.byteLength / 4);
    return renderedSamples;
  };

  var audioFacts = snapshot.audio || {};

  var patchAudio = function (constructor, offline) {
    if (!constructor || !constructor.prototype) return;
    var prototype = constructor.prototype;

    var contextFacts = offline ? audioFacts.offline || {} : audioFacts.context || {};
    var destinationFacts = audioFacts.destination || {};

    var destination = audioNode("destination", {
      maxChannelCount: offline ? destinationFacts.maxChannelCount || 1 : destinationFacts.maxChannelCount || 2,
      channelCount: destinationFacts.channelCount || 2,
      channelCountMode: destinationFacts.channelCountMode || "explicit",
      channelInterpretation: destinationFacts.channelInterpretation || "speakers",
      numberOfInputs: destinationFacts.numberOfInputs || 1,
      numberOfOutputs: destinationFacts.numberOfOutputs || 0,
    });

    patchGetter(prototype, "destination", function () { return destination; });
    patchGetter(prototype, "sampleRate", function () { return contextFacts.sampleRate || 44100; });
    var liveState = contextFacts.state || "suspended";
    patchGetter(prototype, "state", function () { return liveState; });
    patchGetter(prototype, "currentTime", function () { return env.clock() / 1000; });
    patchGetter(prototype, "listener", function () { return audioNode("listener", audioFacts.listener || {}); });

    if (!offline) {
      patchGetter(prototype, "baseLatency", function () { return contextFacts.baseLatency || 0; });
      patchGetter(prototype, "outputLatency", function () { return contextFacts.outputLatency || 0; });
    } else {
      patchGetter(prototype, "length", function () { return contextFacts.length || 44100; });
    }

    patch(prototype, "close", function close() {
      liveState = "closed";
      return Promise.resolve();
    });

    patch(prototype, "resume", function resume() {
      if (liveState !== "closed") liveState = "running";
      return Promise.resolve();
    });

    patch(prototype, "suspend", function suspend() {
      if (liveState !== "closed") liveState = "suspended";
      return Promise.resolve();
    });
    patch(prototype, "createAnalyser", function createAnalyser() {
      return audioNode("analyser", {
        fftSize: 2048,
        frequencyBinCount: 1024,
        minDecibels: -100,
        maxDecibels: -30,
        smoothingTimeConstant: 0.8,
        getFloatFrequencyData: asNative(function getFloatFrequencyData(array) {
          for (var index = 0; index < array.length; index += 1) array[index] = -100;
          return undefined;
        }, "getFloatFrequencyData"),
        getByteFrequencyData: asNative(function getByteFrequencyData() { return undefined; }, "getByteFrequencyData"),
        getFloatTimeDomainData: asNative(function getFloatTimeDomainData() { return undefined; }, "getFloatTimeDomainData"),
      });
    });
    patch(prototype, "createOscillator", function createOscillator() {
      return audioNode("oscillator", {
        type: "sine",
        frequency: audioParam(440),
        detune: audioParam(0),
        start: asNative(function start() { return undefined; }, "start"),
        stop: asNative(function stop() { return undefined; }, "stop"),
      });
    });
    patch(prototype, "createGain", function createGain() {
      return audioNode("gain", { gain: audioParam(1) });
    });
    patch(prototype, "createDynamicsCompressor", function createDynamicsCompressor() {
      return audioNode("compressor", {
        threshold: audioParam(-24),
        knee: audioParam(30),
        ratio: audioParam(12),
        attack: audioParam(0.003),
        release: audioParam(0.25),
        reduction: 0,
      });
    });
    patch(prototype, "createBuffer", function createBuffer(channels, length, rate) {
      return audioBuffer(channels || 1, length || 1, rate || 44100);
    });
    patch(prototype, "createBufferSource", function createBufferSource() {
      return audioNode("bufferSource", {
        buffer: null,
        loop: false,
        playbackRate: audioParam(1),
        start: asNative(function start() { return undefined; }, "start"),
        stop: asNative(function stop() { return undefined; }, "stop"),
      });
    });
    patch(prototype, "createScriptProcessor", function createScriptProcessor() {
      return audioNode("scriptProcessor", { onaudioprocess: null, bufferSize: 4096 });
    });
    patch(prototype, "createChannelMerger", function createChannelMerger() { return audioNode("merger"); });
    patch(prototype, "createChannelSplitter", function createChannelSplitter() { return audioNode("splitter"); });
    patch(prototype, "createStereoPanner", function createStereoPanner() { return audioNode("panner", { pan: audioParam(0) }); });
    patch(prototype, "decodeAudioData", function decodeAudioData() { return Promise.resolve(audioBuffer(1, 1024, 44100)); });

    if (offline) {
      var completeHandlers = new WeakMap();

      try {
        Object.defineProperty(prototype, "oncomplete", {
          get: asNative(function () { return completeHandlers.get(this) || null; }, "get oncomplete"),
          set: asNative(function (handler) {
            if (typeof handler === "function") completeHandlers.set(this, handler);
            else completeHandlers.delete(this);
          }, "set oncomplete"),
          enumerable: true,
          configurable: true,
        });
      } catch (error) {}

      patch(prototype, "startRendering", function startRendering() {
        var context = this;
        var samples = recordedChannel();
        var length = samples ? samples.length : contextFacts.length || 44100;
        var rate = traits.audio ? traits.audio.sampleRate || 44100 : 44100;
        var buffer = audioBuffer(1, length, rate, samples);

        liveState = "running";

        return new Promise(function (resolve) {
          later(function () {
            liveState = "closed";
            env.spend(recordedDuration("audio"));

            var event = makeEvent("complete", { renderedBuffer: buffer });

            try {
              if (typeof context.oncomplete === "function") context.oncomplete(event);
            } catch (error) {}

            try {
              env.dispatch(context, event);
            } catch (error) {}

            resolve(buffer);
          }, 0);
        });
      });
    }
  };

  patchAudio(globalThis.AudioContext, false);
  patchAudio(globalThis.OfflineAudioContext, true);

  var requestInitKeys = [
    "adAuctionHeaders", "attributionReporting", "body", "browsingTopics", "cache", "credentials", "duplex",
    "headers", "integrity", "keepalive", "method", "mode", "priority", "privateToken", "redirect", "referrer",
    "referrerPolicy", "sharedStorageWritable", "signal", "targetAddressSpace",
  ];

  var alsoAsks = { attributionReporting: true };

  var readInit = function (init, keys, asks) {
    var out = {};
    if (init === null || (typeof init !== "object" && typeof init !== "function")) return out;

    for (var index = 0; index < keys.length; index += 1) {
      var key = keys[index];
      var value;

      try {
        value = init[key];
      } catch (error) {
        value = undefined;
      }

      if (asks && asks[key]) {
        try {
          key in init;
        } catch (error) {}
      }

      if (value !== undefined) out[key] = value;
    }

    return out;
  };

  var dictionaries = {
    Request: requestInitKeys,
    UIEvent: ["bubbles", "cancelable", "composed", "detail", "sourceCapabilities", "view"],
    InputEvent: ["bubbles", "cancelable", "composed", "detail", "sourceCapabilities", "view", "data", "dataTransfer", "inputType", "isComposing", "targetRanges"],
  };

  for (var dictName in dictionaries) {
    (function (name, keys) {
      if (typeof globalThis[name] !== "function") return;

      env.behaviour["window." + name] = function (first, second) {
        var init = name === "Request" ? second : second;
        var read = readInit(init, keys, alsoAsks);

        try {
          if (name === "Request") {
            this.url = typeof first === "string" ? first : (first && first.url) || "";
            this.method = read.method || "GET";
          } else {
            this.type = String(first);
          }
        } catch (error) {}

        return undefined;
      };
    })(dictName, dictionaries[dictName]);
  }

  var makeIterator = function (entries) {
    var index = 0;

    var iterator = {
      next: asNative(function next() {
        if (index >= entries.length) return { value: undefined, done: true };
        var value = entries[index];
        index += 1;
        return { value: value, done: false };
      }, "next"),
    };

    Object.defineProperty(iterator, Symbol.iterator, {
      value: asNative(function () { return iterator; }, "[Symbol.iterator]"),
      writable: true,
      configurable: true,
    });

    return iterator;
  };

  var defineIterable = function (holder, entriesOf, extras) {
    if (!holder) return;

    Object.defineProperty(holder, Symbol.iterator, {
      value: asNative(function () { return makeIterator(entriesOf.call(this)); }, "[Symbol.iterator]"),
      writable: true,
      enumerable: false,
      configurable: true,
    });

    if (!extras) return;

    patch(holder, "values", function values() {
      var entries = entriesOf.call(this);
      return makeIterator(extras === "map" ? entries.map(function (pair) { return pair[1]; }) : entries);
    });

    patch(holder, "keys", function keys() {
      var entries = entriesOf.call(this);
      return makeIterator(extras === "map" ? entries.map(function (pair) { return pair[0]; }) : entries);
    });

    patch(holder, "entries", function entries() {
      var found = entriesOf.call(this);
      return makeIterator(extras === "map" ? found : found.map(function (value, index) { return [index, value]; }));
    });

    patch(holder, "forEach", function forEach(visit, self) {
      var found = entriesOf.call(this);

      for (var index = 0; index < found.length; index += 1) {
        if (extras === "map") visit.call(self, found[index][1], found[index][0], this);
        else visit.call(self, found[index], index, this);
      }

      return undefined;
    });
  };

  var indexedEntries = function () {
    var out = [];
    var length = Number(this.length) || 0;
    for (var index = 0; index < length; index += 1) out.push(this[index]);
    return out;
  };

  var iterates = function (holder) {
    try {
      var existing = holder[Symbol.iterator];
      if (typeof existing !== "function") return false;
      var made = existing.call({ length: 0 });
      return Boolean(made) && typeof made.next === "function";
    } catch (error) {
      return false;
    }
  };

  var installIterators = function () {
    var names = Object.getOwnPropertyNames(globalThis);

    for (var index = 0; index < names.length; index += 1) {
      var value;

      try {
        value = globalThis[names[index]];
      } catch (error) {
        continue;
      }

      if (typeof value !== "function" || !value.prototype || !env.isHost(value.prototype)) continue;

      var proto = value.prototype;
      if (!Object.getOwnPropertyDescriptor(proto, "item") && !Object.getOwnPropertyDescriptor(proto, Symbol.iterator)) continue;
      if (iterates(proto)) continue;

      defineIterable(proto, indexedEntries, null);
    }
  };

  if (typeof globalThis.DataTransfer === "function" && globalThis.DataTransferItemList && globalThis.FileList && globalThis.DataTransferItem) {
    var transferOf = new WeakMap();

    var indexList = function (list, entries) {
      var index = 0;

      while (Object.prototype.hasOwnProperty.call(list, String(index))) {
        delete list[String(index)];
        index += 1;
      }

      for (index = 0; index < entries.length; index += 1) {
        Object.defineProperty(list, String(index), { value: entries[index], enumerable: true, configurable: true });
      }

      env.overrides.set(list, { length: entries.length });
    };

    var refreshTransfer = function (state) {
      var files = [];
      var types = [];

      for (var index = 0; index < state.entries.length; index += 1) {
        var entry = state.entries[index];
        if (entry.kind === "file") files.push(entry.data);
        else if (types.indexOf(entry.type) === -1) types.push(entry.type);
      }

      if (files.length && types.indexOf("Files") === -1) types.push("Files");

      indexList(state.items, state.entries.map(function (entry) { return entry.item; }));
      indexList(state.files, files);

      env.overrides.set(state.transfer, {
        dropEffect: "none",
        effectAllowed: "none",
        items: state.items,
        files: state.files,
        types: types,
      });
    };

    env.behaviour["window.DataTransfer"] = function () {
      var state = {
        transfer: this,
        entries: [],
        items: Object.create(globalThis.DataTransferItemList.prototype),
        files: Object.create(globalThis.FileList.prototype),
      };

      transferOf.set(state.items, state);
      refreshTransfer(state);
      return undefined;
    };

    patch(globalThis.DataTransferItemList.prototype, "add", function add(data, type) {
      var state = transferOf.get(this);
      if (!state) return null;

      var isFile = false;

      try {
        isFile = globalThis.File ? data instanceof globalThis.File : false;
      } catch (error) {
        isFile = false;
      }

      var item = Object.create(globalThis.DataTransferItem.prototype);
      var entry = isFile
        ? { kind: "file", type: String(data.type || ""), data: data, item: item }
        : { kind: "string", type: String(type === undefined ? "" : type), data: String(data), item: item };

      env.overrides.set(item, { kind: entry.kind, type: entry.type });
      state.entries.push(entry);
      refreshTransfer(state);

      return item;
    });

    patch(globalThis.DataTransferItemList.prototype, "remove", function remove(index) {
      var state = transferOf.get(this);
      if (!state) return undefined;
      state.entries.splice(Number(index), 1);
      refreshTransfer(state);
      return undefined;
    });

    patch(globalThis.DataTransferItemList.prototype, "clear", function clear() {
      var state = transferOf.get(this);
      if (!state) return undefined;
      state.entries.length = 0;
      refreshTransfer(state);
      return undefined;
    });

    patch(globalThis.FileList.prototype, "item", function item(index) {
      var value = this[String(Number(index))];
      return value === undefined ? null : value;
    });
  }

  var pairsFrom = function (init) {
    var pairs = [];

    if (init === undefined || init === null) return pairs;

    if (typeof init === "string") {
      var text = init.charAt(0) === "?" ? init.slice(1) : init;
      var parts = text.split("&");

      for (var index = 0; index < parts.length; index += 1) {
        if (!parts[index]) continue;
        var at = parts[index].indexOf("=");
        var key = at === -1 ? parts[index] : parts[index].slice(0, at);
        var value = at === -1 ? "" : parts[index].slice(at + 1);

        try {
          pairs.push([decodeURIComponent(key.replace(/\+/g, " ")), decodeURIComponent(value.replace(/\+/g, " "))]);
        } catch (error) {
          pairs.push([key, value]);
        }
      }

      return pairs;
    }

    if (Array.isArray(init)) {
      for (var entry = 0; entry < init.length; entry += 1) pairs.push([String(init[entry][0]), String(init[entry][1])]);
      return pairs;
    }

    if (typeof init === "object") {
      var names = Object.keys(init);
      for (var name = 0; name < names.length; name += 1) pairs.push([names[name], String(init[names[name]])]);
    }

    return pairs;
  };

  var pairState = new WeakMap();
  var pairsOf = function (holder) {
    var found = pairState.get(holder);
    if (!found) {
      found = [];
      pairState.set(holder, found);
    }
    return found;
  };

  var installPairApi = function (constructor, lowercase) {
    if (typeof constructor !== "function" || !constructor.prototype) return;

    var proto = constructor.prototype;
    var normalise = function (name) { return lowercase ? String(name).toLowerCase() : String(name); };

    patch(proto, "append", function append(name, value) {
      pairsOf(this).push([normalise(name), String(value)]);
      return undefined;
    });

    patch(proto, "set", function set(name, value) {
      var pairs = pairsOf(this);
      var key = normalise(name);
      var replaced = false;

      for (var index = pairs.length - 1; index >= 0; index -= 1) {
        if (pairs[index][0] !== key) continue;
        if (replaced) pairs.splice(index, 1);
        else {
          pairs[index][1] = String(value);
          replaced = true;
        }
      }

      if (!replaced) pairs.push([key, String(value)]);
      return undefined;
    });

    patch(proto, "get", function get(name) {
      var pairs = pairsOf(this);
      var key = normalise(name);

      for (var index = 0; index < pairs.length; index += 1) if (pairs[index][0] === key) return pairs[index][1];
      return null;
    });

    patch(proto, "getAll", function getAll(name) {
      var key = normalise(name);
      return pairsOf(this).filter(function (pair) { return pair[0] === key; }).map(function (pair) { return pair[1]; });
    });

    patch(proto, "has", function has(name) {
      var key = normalise(name);
      return pairsOf(this).some(function (pair) { return pair[0] === key; });
    });

    patch(proto, "delete", function remove(name) {
      var key = normalise(name);
      var pairs = pairsOf(this);

      for (var index = pairs.length - 1; index >= 0; index -= 1) if (pairs[index][0] === key) pairs.splice(index, 1);
      return undefined;
    });

    patchGetter(proto, "size", function () { return pairsOf(this).length; });
    defineIterable(proto, function () { return pairsOf(this).map(function (pair) { return [pair[0], pair[1]]; }); }, "map");
  };

  if (typeof globalThis.URLSearchParams === "function") {
    env.behaviour["window.URLSearchParams"] = function (init) {
      pairState.set(this, pairsFrom(init));
      return undefined;
    };

    installPairApi(globalThis.URLSearchParams, false);

    patch(globalThis.URLSearchParams.prototype, "sort", function sort() {
      pairsOf(this).sort(function (left, right) { return left[0] < right[0] ? -1 : left[0] > right[0] ? 1 : 0; });
      return undefined;
    });

    patch(globalThis.URLSearchParams.prototype, "toString", function toString() {
      return pairsOf(this)
        .map(function (pair) { return encodeURIComponent(pair[0]) + "=" + encodeURIComponent(pair[1]); })
        .join("&");
    });
  }


  if (typeof globalThis.URL === "function") {
    var urlState = new WeakMap();

    var splitUrl = function (text) {
      var match = /^([a-zA-Z][a-zA-Z0-9+.-]*:)?(\/\/)?([^/?#]*)?([^?#]*)?(\?[^#]*)?(#.*)?$/.exec(String(text));
      if (!match) return null;

      return {
        scheme: match[1] || "",
        slashes: Boolean(match[2]),
        authority: match[3] || "",
        path: match[4] || "",
        query: match[5] || "",
        hash: match[6] || "",
      };
    };

    var normalisePath = function (path) {
      var parts = String(path).split("/");
      var out = [];

      for (var index = 0; index < parts.length; index += 1) {
        var part = parts[index];
        if (part === ".") continue;

        if (part === "..") {
          if (out.length > 1) out.pop();
          continue;
        }

        out.push(part);
      }

      return out.join("/");
    };

    var defaultPorts = { "http:": "80", "https:": "443", "ws:": "80", "wss:": "443", "ftp:": "21" };

    var buildUrl = function (input, base) {
      var text = String(input).trim();
      var piece = splitUrl(text);
      if (!piece) return null;

      var parent = base === undefined || base === null ? null : buildUrl(base, undefined);
      if (base !== undefined && base !== null && !parent) return null;

      var scheme = piece.scheme;
      var authority = piece.slashes ? piece.authority : "";
      var path = piece.slashes ? piece.path : (piece.authority || "") + piece.path;
      var query = piece.query;
      var hash = piece.hash;

      if (!scheme) {
        if (!parent) return null;
        scheme = parent.protocol;

        if (!piece.slashes) {
          authority = parent.host;

          if (!path) {
            path = parent.pathname;
            if (!query) query = parent.search;
          } else if (path.charAt(0) !== "/") {
            path = parent.pathname.replace(/[^/]*$/, "") + path;
          }
        }
      }

      var special = scheme === "http:" || scheme === "https:" || scheme === "ws:" || scheme === "wss:" || scheme === "ftp:" || scheme === "file:";

      if (special && !authority && !parent) return null;

      var credentials = "";
      var hostPort = authority;
      var at = authority.lastIndexOf("@");

      if (at !== -1) {
        credentials = authority.slice(0, at);
        hostPort = authority.slice(at + 1);
      }

      var username = "";
      var password = "";

      if (credentials) {
        var colon = credentials.indexOf(":");
        username = colon === -1 ? credentials : credentials.slice(0, colon);
        password = colon === -1 ? "" : credentials.slice(colon + 1);
      }

      var hostname = hostPort;
      var port = "";
      var portAt = hostPort.lastIndexOf(":");

      if (portAt !== -1 && hostPort.indexOf("]") < portAt) {
        hostname = hostPort.slice(0, portAt);
        port = hostPort.slice(portAt + 1);
      }

      if (special && port && defaultPorts[scheme] === port) port = "";
      if (special) hostname = hostname.toLowerCase();
      if (special && !path) path = "/";
      if (special && path && path.charAt(0) !== "/") path = "/" + path;

      path = normalisePath(path);

      var host = hostname + (port ? ":" + port : "");
      var origin = special && hostname ? scheme + "//" + host : "null";
      var href = scheme + (special || piece.slashes ? "//" : "") + (username ? username + (password ? ":" + password : "") + "@" : "") + host + path + query + hash;

      return {
        href: href,
        origin: origin,
        protocol: scheme,
        username: username,
        password: password,
        host: host,
        hostname: hostname,
        port: port,
        pathname: path,
        search: query === "?" ? "" : query,
        hash: hash === "#" ? "" : hash,
      };
    };

    var RealUrl = globalThis.URL;

    var UrlShell = function URL(input, base) {
      if (new.target === undefined) {
        throw env.hideFrames(new TypeError("Failed to construct 'URL': Please use the 'new' operator, this DOM object constructor cannot be called as a function."));
      }

      var parsed = buildUrl(input, base);

      if (!parsed) {
        throw env.hideFrames(new TypeError("Failed to construct 'URL': Invalid URL"));
      }

      var made = Object.create(RealUrl.prototype);
      urlState.set(made, parsed);
      return made;
    };

    asNative(UrlShell, "URL");

    try {
      Object.defineProperty(UrlShell, "prototype", { value: RealUrl.prototype, writable: false, enumerable: false, configurable: false });
      Object.defineProperty(UrlShell, "length", { value: 1, configurable: true });
      Object.defineProperty(RealUrl.prototype, "constructor", { value: UrlShell, writable: true, enumerable: false, configurable: true });
    } catch (error) {}

    for (var staticName of Object.getOwnPropertyNames(RealUrl)) {
      if (staticName === "prototype" || staticName === "length" || staticName === "name") continue;

      try {
        Object.defineProperty(UrlShell, staticName, Object.getOwnPropertyDescriptor(RealUrl, staticName));
      } catch (error) {}
    }

    try {
      Object.defineProperty(globalThis, "URL", { value: UrlShell, writable: true, enumerable: false, configurable: true });
      if (globalThis.webkitURL) Object.defineProperty(globalThis, "webkitURL", { value: UrlShell, writable: true, enumerable: false, configurable: true });
    } catch (error) {}

    var objectUrls = new Map();

    patch(UrlShell, "createObjectURL", function createObjectURL(source) {
      if (arguments.length === 0) {
        throw env.hideFrames(new TypeError("Failed to execute 'createObjectURL' on 'URL': 1 argument required, but only 0 present."));
      }

      if (source === null || (typeof source !== "object" && typeof source !== "function")) {
        throw env.hideFrames(new TypeError("Failed to execute 'createObjectURL' on 'URL': Overload resolution failed."));
      }

      var address = "blob:" + String(globalThis.location.origin) + "/" + noiseUuid();
      objectUrls.set(address, source);
      return address;
    });

    patch(UrlShell, "revokeObjectURL", function revokeObjectURL(address) {
      objectUrls.delete(String(address));
      return undefined;
    });

    env.objectUrls = objectUrls;

    var urlFields = ["href", "origin", "protocol", "username", "password", "host", "hostname", "port", "pathname", "search", "hash"];

    for (var urlIndex = 0; urlIndex < urlFields.length; urlIndex += 1) {
      (function (name) {
        patchGetter(globalThis.URL.prototype, name, function () {
          var parsed = urlState.get(this);
          return parsed ? parsed[name] : "";
        });
      })(urlFields[urlIndex]);
    }

    patchGetter(globalThis.URL.prototype, "searchParams", function () {
      var parsed = urlState.get(this);
      return new globalThis.URLSearchParams(parsed ? parsed.search : "");
    });

    patch(globalThis.URL.prototype, "toString", function toString() {
      var parsed = urlState.get(this);
      return parsed ? parsed.href : "";
    });

    patch(globalThis.URL.prototype, "toJSON", function toJSON() {
      var parsed = urlState.get(this);
      return parsed ? parsed.href : "";
    });

    patch(UrlShell, "canParse", function canParse(input, base) {
      return Boolean(buildUrl(input, base));
    });

    patch(UrlShell, "parse", function parse(input, base) {
      if (!buildUrl(input, base)) return null;
      return new globalThis.URL(input, base);
    });

    env.parseUrlText = buildUrl;
  }

  if (globalThis.Animation && globalThis.Animation.prototype && globalThis.KeyframeEffect && globalThis.Element) {
    var effectTiming = new WeakMap();

    var computedTiming = function (timing) {
      var duration = typeof timing.duration === "number" ? timing.duration : 0;
      var activeDuration = duration * timing.iterations;

      return {
        delay: timing.delay,
        endDelay: timing.endDelay,
        fill: timing.fill === "auto" ? "none" : timing.fill,
        iterationStart: timing.iterationStart,
        iterations: timing.iterations,
        duration: timing.duration,
        direction: timing.direction,
        easing: timing.easing,
        endTime: Math.max(timing.delay + activeDuration + timing.endDelay, 0),
        activeDuration: activeDuration,
        localTime: 0,
        progress: 0,
        currentIteration: 0,
      };
    };

    patch(globalThis.KeyframeEffect.prototype, "getComputedTiming", function getComputedTiming() {
      var timing = effectTiming.get(this);
      return timing ? computedTiming(timing) : undefined;
    });

    patch(globalThis.KeyframeEffect.prototype, "getTiming", function getTiming() {
      var timing = effectTiming.get(this);
      if (!timing) return undefined;

      return {
        delay: timing.delay,
        endDelay: timing.endDelay,
        fill: timing.fill,
        iterationStart: timing.iterationStart,
        iterations: timing.iterations,
        duration: timing.duration,
        direction: timing.direction,
        easing: timing.easing,
      };
    });

    patch(globalThis.KeyframeEffect.prototype, "getKeyframes", function getKeyframes() {
      var timing = effectTiming.get(this);
      return timing ? timing.keyframes.slice() : [];
    });

    patch(globalThis.Animation.prototype, "play", function play() { return undefined; });
    patch(globalThis.Animation.prototype, "pause", function pause() { return undefined; });
    patch(globalThis.Animation.prototype, "cancel", function cancel() { return undefined; });
    patch(globalThis.Animation.prototype, "finish", function finish() { return undefined; });
    patch(globalThis.Animation.prototype, "reverse", function reverse() { return undefined; });

    patch(globalThis.Element.prototype, "animate", function animate(keyframes, options) {
      var timing = {
        delay: 0,
        endDelay: 0,
        fill: "auto",
        iterationStart: 0,
        iterations: 1,
        duration: "auto",
        direction: "normal",
        easing: "linear",
        keyframes: Array.isArray(keyframes) ? keyframes : [],
      };

      if (typeof options === "number") {
        timing.duration = options;
      } else if (options !== null && typeof options === "object") {
        var fields = ["delay", "endDelay", "fill", "iterationStart", "iterations", "duration", "direction", "easing"];
        for (var index = 0; index < fields.length; index += 1) {
          if (options[fields[index]] !== undefined) timing[fields[index]] = options[fields[index]];
        }
      }

      var effect = Object.create(globalThis.KeyframeEffect.prototype);
      effectTiming.set(effect, timing);

      env.overrides.set(effect, { target: this, pseudoElement: null, composite: "replace" });

      var animation = Object.create(globalThis.Animation.prototype);
      var timeline = globalThis.DocumentTimeline ? Object.create(globalThis.DocumentTimeline.prototype) : null;

      if (timeline) env.overrides.set(timeline, { currentTime: env.clock() });

      env.overrides.set(animation, {
        effect: effect,
        timeline: timeline,
        startTime: null,
        currentTime: 0,
        overallProgress: 0,
        playbackRate: 1,
        playState: "running",
        replaceState: "active",
        pending: false,
        id: "",
        finished: Promise.resolve(animation),
        ready: Promise.resolve(animation),
      });

      return animation;
    });

    patch(globalThis.Document.prototype, "getAnimations", function getAnimations() { return []; });
  }

  if (globalThis.SpeechSynthesis && globalThis.SpeechSynthesis.prototype && globalThis.SpeechSynthesisVoice) {
    var voiceList = null;

    patch(globalThis.SpeechSynthesis.prototype, "getVoices", function getVoices() {
      if (voiceList) return voiceList.slice();

      var recorded = env.recordGraphics ? env.recordGraphics("window", "getVoices", []) : null;
      var described = [];

      if (typeof recorded === "string" && recorded) {
        try {
          described = JSON.parse(recorded);
        } catch (error) {
          described = [];
        }
      }

      voiceList = [];

      for (var index = 0; index < described.length; index += 1) {
        var voice = Object.create(globalThis.SpeechSynthesisVoice.prototype);

        env.overrides.set(voice, {
          voiceURI: described[index].voiceURI,
          name: described[index].name,
          lang: described[index].lang,
          localService: described[index].localService,
          default: described[index].default,
        });

        voiceList.push(voice);
      }

      return voiceList.slice();
    });
  }

  if (globalThis.FontFaceSet && globalThis.FontFaceSet.prototype) {
    var fontFaces = new WeakMap();
    var facesOf = function (holder) {
      var found = fontFaces.get(holder);
      if (!found) {
        found = [];
        fontFaces.set(holder, found);
      }
      return found;
    };

    var fontProto = globalThis.FontFaceSet.prototype;

    patch(fontProto, "add", function add(face) {
      var faces = facesOf(this);
      if (faces.indexOf(face) === -1) faces.push(face);
      return this;
    });

    patch(fontProto, "delete", function remove(face) {
      var faces = facesOf(this);
      var at = faces.indexOf(face);
      if (at === -1) return false;
      faces.splice(at, 1);
      return true;
    });

    patch(fontProto, "clear", function clear() {
      facesOf(this).length = 0;
      return undefined;
    });

    patch(fontProto, "has", function has(face) { return facesOf(this).indexOf(face) !== -1; });
    patch(fontProto, "check", function check() { return true; });
    patch(fontProto, "load", function load() { return Promise.resolve([]); });
    patchGetter(fontProto, "size", function () { return facesOf(this).length; });
    patchGetter(fontProto, "status", function () { return "loaded"; });
    patchGetter(fontProto, "ready", function () { return Promise.resolve(this); });

    defineIterable(fontProto, function () { return facesOf(this).slice(); }, "set");
  }

  if (globalThis.DOMTokenList && globalThis.DOMTokenList.prototype && globalThis.Element && globalThis.Element.prototype) {
    var tokenOwner = new WeakMap();
    var tokenLists = new WeakMap();

    var tokensOf = function (list) {
      var owner = tokenOwner.get(list);
      var text = owner ? String(owner.getAttribute("class") || "") : "";
      return text.split(/\s+/).filter(function (token) { return token.length > 0; });
    };

    var writeTokens = function (list, tokens) {
      var owner = tokenOwner.get(list);
      if (owner) owner.setAttribute("class", tokens.join(" "));
    };

    var tokenProto = globalThis.DOMTokenList.prototype;

    patch(tokenProto, "contains", function contains(token) { return tokensOf(this).indexOf(String(token)) !== -1; });
    patch(tokenProto, "item", function item(index) {
      var tokens = tokensOf(this);
      return tokens[Number(index)] === undefined ? null : tokens[Number(index)];
    });

    patch(tokenProto, "add", function add() {
      var tokens = tokensOf(this);
      for (var index = 0; index < arguments.length; index += 1) if (tokens.indexOf(String(arguments[index])) === -1) tokens.push(String(arguments[index]));
      writeTokens(this, tokens);
      return undefined;
    });

    patch(tokenProto, "remove", function remove() {
      var tokens = tokensOf(this);
      for (var index = 0; index < arguments.length; index += 1) {
        var at = tokens.indexOf(String(arguments[index]));
        if (at !== -1) tokens.splice(at, 1);
      }
      writeTokens(this, tokens);
      return undefined;
    });

    patch(tokenProto, "toggle", function toggle(token) {
      var tokens = tokensOf(this);
      var at = tokens.indexOf(String(token));

      if (at === -1) tokens.push(String(token));
      else tokens.splice(at, 1);

      writeTokens(this, tokens);
      return at === -1;
    });

    patch(tokenProto, "replace", function replace(from, to) {
      var tokens = tokensOf(this);
      var at = tokens.indexOf(String(from));
      if (at === -1) return false;
      tokens[at] = String(to);
      writeTokens(this, tokens);
      return true;
    });

    patch(tokenProto, "supports", function supports() { return true; });
    patch(tokenProto, "toString", function toString() { return tokensOf(this).join(" "); });
    patchGetter(tokenProto, "length", function () { return tokensOf(this).length; });
    patchGetter(tokenProto, "value", function () { return tokensOf(this).join(" "); });
    defineIterable(tokenProto, function () { return tokensOf(this); }, "list");

    patchGetter(globalThis.Element.prototype, "classList", function () {
      var existing = tokenLists.get(this);
      if (existing) return existing;

      var list = Object.create(tokenProto);
      tokenOwner.set(list, this);
      tokenLists.set(this, list);
      return list;
    });
  }

  if (typeof globalThis.FormData === "function") {
    env.behaviour["window.FormData"] = function () {
      pairState.set(this, []);
      return undefined;
    };

    installPairApi(globalThis.FormData, false);
  }

  if (typeof globalThis.Headers === "function") {
    env.behaviour["window.Headers"] = function (init) {
      if (init !== null && (typeof init === "object" || typeof init === "function")) {
        try {
          init[Symbol.iterator];
        } catch (error) {}
      }

      pairState.set(this, pairsFrom(init).map(function (pair) { return [String(pair[0]).toLowerCase(), pair[1]]; }));
      return undefined;
    };

    installPairApi(globalThis.Headers, true);
  }

  globalThis.fetch = asNative(function fetch(input, init) {
    var url = typeof input === "string" ? input : input && input.url;
    var read = readInit(init, requestInitKeys, alsoAsks);
    var method = read.method || "GET";
    var body = read.body;
    var headers = read.headers;

    record("fetch", { url: url, method: method });

    return new Promise(function (resolve) {
      bridge.request(
        method,
        String(url),
        JSON.stringify(headers || {}),
        body,
        function (status, headerJson, text) {
          var headers = JSON.parse(headerJson);

          resolve({
            ok: status >= 200 && status < 300,
            status: status,
            headers: {
              get: function (name) {
                var value = headers[String(name).toLowerCase()];
                return value === undefined ? null : value;
              },
            },
            text: function () { return Promise.resolve(text); },
            json: function () { return Promise.resolve(JSON.parse(text)); },
          });
        },
      );
    });
  }, "fetch");

  var embedderWindow = null;

  try {
    embedderWindow = bridge.embedder ? bridge.embedder() : null;
  } catch (error) {}

  try {
    Object.defineProperty(globalThis, "frameElement", {
      get: asNative(function () {
        try {
          return bridge.frameElement ? bridge.frameElement() : null;
        } catch (error) {
          return null;
        }
      }, "get frameElement"),
      enumerable: true,
      configurable: true,
    });
  } catch (error) {}

  var referrerHref = "";

  try {
    referrerHref = String(bridge.referrer() || "");
  } catch (error) {}

  var parent = embedderWindow || (referrerHref ? Object.create(protoOf(globalThis) || Object.prototype) : globalThis);

  var embedder = (function () {
    var href = "";

    try {
      href = String(bridge.referrer() || "");
    } catch (error) {}

    var match = href.match(/^(https?:\/\/([^:/]+)(?::(\d+))?)(\/[^?#]*)?(\?[^#]*)?(#.*)?$/);
    if (!match) return null;

    return {
      href: href,
      origin: match[1],
      hostname: match[2],
      port: match[3] || "",
      pathname: match[4] || "/",
      search: match[5] || "",
      hash: match[6] || "",
      protocol: match[1].split(":")[0] + ":",
      host: match[2] + (match[3] ? ":" + match[3] : ""),
    };
  })();

  var ourOrigin = (function () {
    try {
      return String(globalThis.location.origin);
    } catch (error) {
      return "";
    }
  })();

  var frameLocation = {};

  if (embedder && !embedderWindow) {
    var acrossOrigins = embedder.origin !== ourOrigin;
    var locationFields = ["href", "origin", "protocol", "host", "hostname", "port", "pathname", "search", "hash"];

    for (var lf = 0; lf < locationFields.length; lf += 1) {
      (function (field) {
        try {
          Object.defineProperty(frameLocation, field, {
            get: asNative(function () {
              if (!acrossOrigins) return embedder[field];

              var error = new Error(
                "Failed to read a named property '" + field + "' from 'Location': Blocked a frame with origin \"" +
                  ourOrigin +
                  "\" from accessing a cross-origin frame.",
              );

              error.name = "SecurityError";
              throw env.hideFrames(error);
            }, "get " + field),
            enumerable: true,
            configurable: true,
          });
        } catch (error) {}
      })(locationFields[lf]);
    }

    try {
      Object.defineProperty(parent, "location", { value: frameLocation, writable: false, enumerable: true, configurable: true });
    } catch (error) {}
  }

  patch(parent, "postMessage", function postMessage(message, origin) {
    refuseUncloneable(message, "Window");
    record("postMessage", { message: typeof message === "string" ? message : "[object]", origin: origin });

    try {
      if (parent !== globalThis && bridge.deliverMessage) bridge.deliverMessage(parent, message, String(origin ?? "*"), globalThis);
    } catch (error) {}
  });

  try {
    var chromeApp = globalThis.chrome && globalThis.chrome.app;

    if (chromeApp) {
      var extensionApi = function (name, answer) {
        var existing = chromeApp[name];
        if (typeof existing !== "function") return;

        patch(chromeApp, name, function () {
          if (arguments.length !== 0) throw env.hideFrames(new TypeError("Error in invocation of app." + name + "(): "));
          return answer;
        });
      };

      extensionApi("runningState", "cannot_run");
      extensionApi("getIsInstalled", false);
    }
  } catch (error) {}

  patch(parent, "addEventListener", addListener);
  patch(parent, "removeEventListener", removeListener);

  try {
    Object.defineProperty(globalThis, "parent", { value: parent, writable: false, configurable: true });
    Object.defineProperty(globalThis, "top", { value: parent, writable: false, configurable: true });
    Object.defineProperty(globalThis, "self", { value: globalThis, writable: false, configurable: true });
    Object.defineProperty(globalThis, "frames", { value: globalThis, writable: false, configurable: true });
    Object.defineProperty(globalThis, "window", { value: globalThis, writable: false, configurable: true });
  } catch (error) {}

  var CLONEABLE = {
    Object: true, Array: true, Date: true, RegExp: true, Map: true, Set: true, ArrayBuffer: true,
    SharedArrayBuffer: true, DataView: true, Blob: true, File: true, FileList: true, ImageData: true,
    ImageBitmap: true, Error: true, EvalError: true, RangeError: true, ReferenceError: true,
    SyntaxError: true, TypeError: true, URIError: true, AggregateError: true, DOMException: true,
    Boolean: true, Number: true, String: true, BigInt: true, Int8Array: true, Uint8Array: true,
    Uint8ClampedArray: true, Int16Array: true, Uint16Array: true, Int32Array: true, Uint32Array: true,
    Float16Array: true, Float32Array: true, Float64Array: true, BigInt64Array: true, BigUint64Array: true,
    CryptoKey: true, DOMPoint: true, DOMRect: true, DOMMatrix: true, DOMQuad: true, DOMPointReadOnly: true,
    DOMRectReadOnly: true, DOMMatrixReadOnly: true,
  };

  var cloneLabel = function (value, seen, depth) {
    if (value === null || value === undefined) return null;

    var kind = typeof value;

    if (kind === "function") return String(value);
    if (kind === "symbol") return String(value);
    if (kind !== "object") return null;
    if (depth > 8 || seen.indexOf(value) !== -1) return null;

    seen.push(value);

    var brand = String(Object.prototype.toString.call(value)).slice(8, -1);

    if (brand === "Window" || brand === "global") return "#<Window>";
    if (brand === "Promise") return "#<Promise>";
    if (brand === "WeakMap" || brand === "WeakSet" || brand === "WeakRef") return "#<" + brand + ">";

    if (!CLONEABLE[brand]) return brand + " object";

    if (brand === "Array" || brand === "Object") {
      var keys;

      try {
        keys = Object.keys(value);
      } catch (error) {
        keys = [];
      }

      for (var index = 0; index < keys.length && index < 512; index += 1) {
        var found;

        try {
          found = cloneLabel(value[keys[index]], seen, depth + 1);
        } catch (error) {
          found = null;
        }

        if (found) return found;
      }
    }

    return null;
  };

  var refuseUncloneable = function (message, where, method) {
    var label = cloneLabel(message, [], 0);
    if (!label) return;

    var error = new Error("Failed to execute '" + (method || "postMessage") + "' on '" + where + "': " + label + " could not be cloned.");
    error.name = "DataCloneError";
    throw env.hideFrames(error);
  };

  if (typeof globalThis.structuredClone === "function") {
    patch(globalThis, "structuredClone", function structuredClone(value) {
      refuseUncloneable(value, "Window", "structuredClone");
      return value;
    });
  }

  patch(globalThis, "postMessage", function postMessage(message, origin) {
    refuseUncloneable(message, "Window");
    record("selfMessage", { message: typeof message === "string" ? message : "[object]", origin: origin });

    var payload = message;
    var from = String(origin ?? "*");

    env.later(function () { env.deliverMessage(payload, from, globalThis); }, 0);
  });

  env.deliverMessage = function (message, origin, source) {
    var event = makeEvent("message");

    event.data = message;
    event.origin = origin === "*" ? (globalThis.location ? globalThis.location.origin : "") : origin;
    event.source = source ?? null;
    event.lastEventId = "";
    event.ports = [];

    try {
      dispatch(globalThis, event);
    } catch (error) {
      record("messageError", { message: String(error && error.message) });
    }
  };

  var mediaNumber = function (name) {
    var dpr = Number(globalThis.devicePixelRatio) || 1;
    var screenWidth = Number(globalThis.screen && globalThis.screen.width) || 0;
    var screenHeight = Number(globalThis.screen && globalThis.screen.height) || 0;
    var viewWidth = Number(globalThis.innerWidth) || 0;
    var viewHeight = Number(globalThis.innerHeight) || 0;
    var depth = Number(globalThis.screen && globalThis.screen.colorDepth) || 24;

    switch (name) {
      case "width": return viewWidth;
      case "height": return viewHeight;
      case "device-width": return screenWidth;
      case "device-height": return screenHeight;
      case "aspect-ratio": return viewHeight ? viewWidth / viewHeight : 0;
      case "device-aspect-ratio": return screenHeight ? screenWidth / screenHeight : 0;
      case "resolution": return dpr;
      case "device-pixel-ratio":
      case "-webkit-device-pixel-ratio": return dpr;
      case "color": return Math.floor(depth / 3);
      case "color-index": return 0;
      case "monochrome": return 0;
      case "grid": return 0;
      default: return null;
    }
  };

  var mediaKeyword = {
    orientation: "landscape",
    "prefers-color-scheme": "light",
    "prefers-reduced-motion": "no-preference",
    "prefers-reduced-transparency": "no-preference",
    "prefers-reduced-data": "no-preference",
    "prefers-contrast": "no-preference",
    "forced-colors": "none",
    "inverted-colors": "none",
    hover: "hover",
    "any-hover": "hover",
    pointer: "fine",
    "any-pointer": "fine",
    update: "fast",
    scripting: "enabled",
    "display-mode": "browser",
    "overflow-block": "scroll",
    "overflow-inline": "scroll",
    "dynamic-range": "high",
    "color-gamut": "p3",
  };

  var gamutRank = { srgb: 1, p3: 2, rec2020: 3 };
  var rangeRank = { standard: 1, high: 2 };

  var mediaValue = function (text) {
    var value = String(text).trim().toLowerCase();
    var ratio = value.match(/^(\d+(?:\.\d+)?)\s*\/\s*(\d+(?:\.\d+)?)$/);
    if (ratio) return Number(ratio[1]) / Number(ratio[2]);

    var unit = value.match(/^(-?\d+(?:\.\d+)?)(px|em|rem|pt|dppx|x|dpi|dpcm|cm|mm|in)?$/);
    if (!unit) return null;

    var number = Number(unit[1]);

    switch (unit[2]) {
      case "em":
      case "rem": return number * 16;
      case "pt": return number * (96 / 72);
      case "cm": return number * (96 / 2.54);
      case "mm": return number * (96 / 25.4);
      case "in": return number * 96;
      case "dpi": return number / 96;
      case "dpcm": return number / (96 / 2.54);
      default: return number;
    }
  };

  var degenerateRatio = function (feature) {
    if (feature === "aspect-ratio") return !(Number(globalThis.innerWidth) && Number(globalThis.innerHeight));
    if (feature === "device-aspect-ratio") return !(Number(globalThis.screen && globalThis.screen.width) && Number(globalThis.screen && globalThis.screen.height));
    return false;
  };

  var matchesFeature = function (name, value) {
    var bound = name.match(/^(min|max)-(.+)$/);
    var feature = bound ? bound[2] : name;
    var numeric = mediaNumber(feature);

    if (degenerateRatio(feature)) return true;

    if (numeric !== null) {
      if (value === null) return numeric !== 0;
      var wanted = mediaValue(value);
      if (wanted === null) return false;
      if (bound && bound[1] === "min") return numeric >= wanted;
      if (bound && bound[1] === "max") return numeric <= wanted;
      return numeric === wanted;
    }

    if (Object.prototype.hasOwnProperty.call(mediaKeyword, feature)) {
      var actual = mediaKeyword[feature];

      if (feature === "orientation") {
        var viewWide = (Number(globalThis.innerWidth) || 0) > (Number(globalThis.innerHeight) || 0);
        actual = viewWide ? "landscape" : "portrait";
      }
      if (value === null) return actual !== "none" && actual !== "no-preference";

      var asked = String(value).trim().toLowerCase();
      if (feature === "color-gamut") return (gamutRank[asked] || 9) <= (gamutRank[actual] || 0);
      if (feature === "dynamic-range") return (rangeRank[asked] || 9) <= (rangeRank[actual] || 0);
      return asked === actual;
    }

    return null;
  };

  var evaluateQuery = function (text) {
    var query = String(text).trim().toLowerCase();
    if (!query) return false;

    var negated = false;

    if (/^not\s+/.test(query)) {
      negated = true;
      query = query.replace(/^not\s+/, "");
    } else {
      query = query.replace(/^only\s+/, "");
    }

    var type = query.match(/^(all|screen|print|speech)\b/);

    if (type) {
      query = query.slice(type[0].length).replace(/^\s*and\s*/, "");
      if (type[1] === "print" || type[1] === "speech") return negated;
    }

    var result = true;
    var seen = false;
    var parts = query.split(/\s+and\s+/);

    for (var index = 0; index < parts.length; index += 1) {
      var part = parts[index].trim();
      if (!part) continue;

      var inner = part.match(/^\((.*)\)$/);
      if (!inner) return null;

      var body = inner[1].trim();
      var pair = body.match(/^([-a-z0-9]+)\s*:\s*(.+)$/);
      var answer;

      if (pair) {
        answer = matchesFeature(pair[1], pair[2]);
      } else if (/^[-a-z0-9]+$/.test(body)) {
        answer = matchesFeature(body, null);
      } else {
        var range = body.match(/^([-a-z0-9.\/]+)\s*(<=|>=|<|>|=)\s*([-a-z0-9.\/]+)$/);

        if (!range) return null;

        var left = mediaNumber(range[1]);
        var right = left === null ? mediaNumber(range[3]) : null;
        var feature = left === null ? range[3] : range[1];

        if (degenerateRatio(feature)) {
          seen = true;
          continue;
        }

        var against = mediaValue(left === null ? range[1] : range[3]);
        var actual = left === null ? right : left;

        if (actual === null || against === null) return null;

        var operator = range[2];
        if (left === null) operator = operator === "<=" ? ">=" : operator === ">=" ? "<=" : operator === "<" ? ">" : operator === ">" ? "<" : operator;

        answer =
          operator === "<=" ? actual <= against :
          operator === ">=" ? actual >= against :
          operator === "<" ? actual < against :
          operator === ">" ? actual > against : actual === against;
      }

      if (answer === null) return null;
      seen = true;
      result = result && answer;
    }

    if (!seen && !type) return null;
    return negated ? !result : result;
  };

  var preferenceQuery = /prefers-|forced-colors|inverted-colors|color-gamut|dynamic-range|hover|pointer|update|scripting|display-mode|monochrome|overflow-|video-dynamic-range/;

  var mediaMatches = function (query) {
    var text = String(query);

    if (preferenceQuery.test(text) && env.recordGraphics) {
      var replayed = env.recordGraphics("window", "matchMedia", [text]);
      if (replayed && typeof replayed.matches === "boolean") return replayed.matches;
    }

    var parts = text.split(",");
    var known = false;
    var matched = false;

    for (var index = 0; index < parts.length; index += 1) {
      var answer = evaluateQuery(parts[index]);
      if (answer === null) continue;
      known = true;
      if (answer) matched = true;
    }

    if (known) return matched;

    var recorded = env.recordGraphics ? env.recordGraphics("window", "matchMedia", [text]) : undefined;
    return recorded && typeof recorded.matches === "boolean" ? recorded.matches : false;
  };

  if (globalThis.Intl && globalThis.Intl.PluralRules) {
    var pluralOptions = globalThis.Intl.PluralRules.prototype.resolvedOptions;

    patch(globalThis.Intl.PluralRules.prototype, "resolvedOptions", function resolvedOptions() {
      var out = pluralOptions.call(this);
      if (!out || out.notation !== undefined) return out;

      var ordered = {};

      for (var key in out) {
        ordered[key] = out[key];
        if (key === "type") ordered.notation = "standard";
      }

      return ordered;
    });
  }

  if (globalThis.navigator && globalThis.navigator.plugins && globalThis.navigator.mimeTypes) {
    try {
      var types = [];

      for (var m = 0; m < globalThis.navigator.mimeTypes.length; m += 1) types.push(globalThis.navigator.mimeTypes[m]);

      for (var pi = 0; pi < globalThis.navigator.plugins.length; pi += 1) {
        var plugin = globalThis.navigator.plugins[pi];

        for (var ti = 0; ti < types.length; ti += 1) {
          Object.defineProperty(plugin, String(ti), { value: types[ti], writable: false, enumerable: true, configurable: true });
        }
      }
    } catch (error) {}
  }

  var permissionStates = {
    accelerometer: "granted",
    "ambient-light-sensor": "prompt",
    "background-fetch": "prompt",
    "background-sync": "granted",
    "bluetooth-le-scan": "prompt",
    camera: "prompt",
    "clipboard-read": "prompt",
    "clipboard-write": "granted",
    "display-capture": "prompt",
    geolocation: "prompt",
    gyroscope: "granted",
    "idle-detection": "prompt",
    "local-fonts": "prompt",
    magnetometer: "granted",
    microphone: "prompt",
    midi: "granted",
    "nfc": "prompt",
    notifications: "prompt",
    "payment-handler": "prompt",
    "periodic-background-sync": "prompt",
    "persistent-storage": "prompt",
    push: "prompt",
    "screen-wake-lock": "prompt",
    "storage-access": "prompt",
    "system-wake-lock": "prompt",
    "top-level-storage-access": "prompt",
    "window-management": "prompt",
    "captured-surface-control": "prompt",
  };

  if (globalThis.navigator && globalThis.navigator.permissions) {
    var statusPrototype = globalThis.PermissionStatus && globalThis.PermissionStatus.prototype;

    var makeStatus = function (name, state) {
      var status = statusPrototype ? Object.create(statusPrototype) : {};

      try {
        Object.defineProperty(status, "state", { get: asNative(function () { return state; }, "get state"), configurable: true, enumerable: true });
        Object.defineProperty(status, "name", { get: asNative(function () { return name; }, "get name"), configurable: true, enumerable: true });
        Object.defineProperty(status, "onchange", { value: null, writable: true, configurable: true, enumerable: true });
      } catch (error) {}

      if (!statusPrototype) {
        status.addEventListener = addListener;
        status.removeEventListener = removeListener;
      }

      return status;
    };

    patch(globalThis.navigator.permissions, "query", function query(descriptor) {
      var name = descriptor && descriptor.name !== undefined ? String(descriptor.name) : "";

      if (!Object.prototype.hasOwnProperty.call(permissionStates, name)) {
        return Promise.reject(
          env.hideFrames(new TypeError(
            "Failed to execute 'query' on 'Permissions': Failed to read the 'name' property from 'PermissionDescriptor': The provided value '" +
              name +
              "' is not a valid enum value of type PermissionName.",
          )),
        );
      }

      return Promise.resolve(makeStatus(name, permissionStates[name]));
    });
  }

  var webrtcFacts = traits.webrtc && traits.webrtc.localSdp ? traits.webrtc : null;

  if (webrtcFacts && globalThis.RTCPeerConnection) {
    var rtcSeed = 0x9e3779b9;

    var rtcRandom = function () {
      rtcSeed ^= rtcSeed << 13;
      rtcSeed ^= rtcSeed >>> 17;
      rtcSeed ^= rtcSeed << 5;
      return (rtcSeed >>> 0) / 4294967296;
    };

    var rtcPick = function (alphabet, length) {
      var out = "";
      for (var index = 0; index < length; index += 1) out += alphabet.charAt(Math.floor(rtcRandom() * alphabet.length));
      return out;
    };

    var ICE_ALPHABET = "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    var HEX_ALPHABET = "0123456789ABCDEF";

    var freshSdp = function () {
      var session = String(Math.floor(rtcRandom() * 9) + 1);
      for (var digit = 1; digit < 19; digit += 1) session += String(Math.floor(rtcRandom() * 10));

      var fingerprint = [];
      for (var octet = 0; octet < 32; octet += 1) fingerprint.push(rtcPick(HEX_ALPHABET, 2));

      return String(webrtcFacts.localSdp)
        .replace(/o=- \d+ /, "o=- " + session + " ")
        .replace(/a=ice-ufrag:[^\r\n]*/g, "a=ice-ufrag:" + rtcPick(ICE_ALPHABET, 4))
        .replace(/a=ice-pwd:[^\r\n]*/g, "a=ice-pwd:" + rtcPick(ICE_ALPHABET, 24))
        .replace(/a=fingerprint:sha-256 [^\r\n]*/g, "a=fingerprint:sha-256 " + fingerprint.join(":"));
    };

    var descriptionPrototype = globalThis.RTCSessionDescription && globalThis.RTCSessionDescription.prototype;

    var makeDescription = function (type, sdp) {
      var description = descriptionPrototype ? Object.create(descriptionPrototype) : {};

      try {
        Object.defineProperty(description, "type", { get: asNative(function () { return type; }, "get type"), configurable: true, enumerable: true });
        Object.defineProperty(description, "sdp", { get: asNative(function () { return sdp; }, "get sdp"), configurable: true, enumerable: true });
        Object.defineProperty(description, "toJSON", { value: asNative(function toJSON() { return { type: type, sdp: sdp }; }, "toJSON"), writable: true, configurable: true, enumerable: true });
      } catch (error) {}

      return description;
    };

    var rtcStates = new WeakMap();

    var rtcStateOf = function (connection) {
      var state = rtcStates.get(connection);

      if (!state) {
        state = { local: null, remote: null, signaling: "stable", closed: false };
        rtcStates.set(connection, state);
      }

      return state;
    };

    var rtcPrototype = globalThis.RTCPeerConnection.prototype;

    var rtcReader = function (name, read) {
      try {
        var previous = Object.getOwnPropertyDescriptor(rtcPrototype, name);

        Object.defineProperty(rtcPrototype, name, {
          get: asNative(function () { return read(rtcStateOf(this)); }, "get " + name),
          enumerable: previous ? previous.enumerable : true,
          configurable: true,
        });
      } catch (error) {}
    };

    rtcReader("localDescription", function (state) { return state.closed ? null : state.local; });
    rtcReader("currentLocalDescription", function (state) { return state.closed ? null : state.local; });
    rtcReader("pendingLocalDescription", function (state) { return state.closed ? null : state.local; });
    rtcReader("remoteDescription", function (state) { return state.remote; });
    rtcReader("currentRemoteDescription", function (state) { return state.remote; });
    rtcReader("pendingRemoteDescription", function (state) { return state.remote; });
    rtcReader("signalingState", function (state) { return state.closed ? "closed" : state.signaling; });
    rtcReader("iceGatheringState", function (state) { return state.closed ? "closed" : webrtcFacts.iceGatheringState || "new"; });
    rtcReader("iceConnectionState", function (state) { return state.closed ? "closed" : "new"; });
    rtcReader("connectionState", function (state) { return state.closed ? "closed" : webrtcFacts.connectionState || "new"; });
    rtcReader("canTrickleIceCandidates", function () { return null; });
    rtcReader("sctp", function () { return null; });

    var channelPrototype = globalThis.RTCDataChannel && globalThis.RTCDataChannel.prototype;

    patch(rtcPrototype, "createDataChannel", function createDataChannel(label) {
      var channel = channelPrototype ? Object.create(channelPrototype) : {};
      var name = label === undefined ? "" : String(label);

      try {
        Object.defineProperty(channel, "label", { get: asNative(function () { return name; }, "get label"), configurable: true, enumerable: true });
        Object.defineProperty(channel, "readyState", { get: asNative(function () { return "connecting"; }, "get readyState"), configurable: true, enumerable: true });
      } catch (error) {}

      if (!channelPrototype) {
        channel.close = asNative(function close() { return undefined; }, "close");
        channel.addEventListener = addListener;
        channel.removeEventListener = removeListener;
      }

      return channel;
    });

    patch(rtcPrototype, "createOffer", function createOffer() {
      env.spend(recordedDuration("webrtc"));
      return Promise.resolve(makeDescription("offer", freshSdp()));
    });

    patch(rtcPrototype, "createAnswer", function createAnswer() {
      return Promise.resolve(makeDescription("answer", freshSdp()));
    });

    patch(rtcPrototype, "setLocalDescription", function setLocalDescription(description) {
      var state = rtcStateOf(this);
      var type = description && description.type !== undefined ? String(description.type) : "offer";
      var sdp = description && description.sdp !== undefined ? String(description.sdp) : freshSdp();

      state.local = makeDescription(type, sdp);
      state.signaling = type === "offer" ? "have-local-offer" : "stable";

      return Promise.resolve(undefined);
    });

    patch(rtcPrototype, "setRemoteDescription", function setRemoteDescription(description) {
      var state = rtcStateOf(this);
      var type = description && description.type !== undefined ? String(description.type) : "answer";
      var sdp = description && description.sdp !== undefined ? String(description.sdp) : "";

      state.remote = makeDescription(type, sdp);
      state.signaling = type === "offer" ? "have-remote-offer" : "stable";

      return Promise.resolve(undefined);
    });

    patch(rtcPrototype, "addIceCandidate", function addIceCandidate() { return Promise.resolve(undefined); });
    patch(rtcPrototype, "getStats", function getStats() { return Promise.resolve(new Map()); });
    patch(rtcPrototype, "getSenders", function getSenders() { return []; });
    patch(rtcPrototype, "getReceivers", function getReceivers() { return []; });
    patch(rtcPrototype, "getTransceivers", function getTransceivers() { return []; });
    patch(rtcPrototype, "getConfiguration", function getConfiguration() {
      var recorded = webrtcFacts.configuration || {};
      var copy = {};
      var names = Object.keys(recorded);

      for (var index = 0; index < names.length; index += 1) copy[names[index]] = recorded[names[index]];

      return copy;
    });

    patch(rtcPrototype, "setConfiguration", function setConfiguration() { return undefined; });
    patch(rtcPrototype, "restartIce", function restartIce() { return undefined; });

    patch(rtcPrototype, "close", function close() {
      var state = rtcStateOf(this);
      state.closed = true;
      state.signaling = "closed";
      return undefined;
    });
  }

  if (globalThis.CSS && typeof globalThis.CSS === "object") {
    var known = snapshot.computedStyle || {};

    var supported = function (property, value) {
      var name = String(property).trim().toLowerCase();
      if (!name || String(value).trim() === "") return false;
      if (Object.prototype.hasOwnProperty.call(known, name)) return true;
      return /^(-webkit-|-moz-|--)/.test(name);
    };

    patch(globalThis.CSS, "supports", function supports(property, value) {
      if (arguments.length >= 2) return supported(property, value);

      var condition = String(property).trim();
      var pair = condition.match(/^\((.+?)\s*:\s*(.+)\)$/);
      if (pair) return supported(pair[1], pair[2]);

      var bare = condition.match(/^(.+?)\s*:\s*(.+)$/);
      return bare ? supported(bare[1], bare[2]) : false;
    });

    patch(globalThis.CSS, "escape", function escape(value) {
      return String(value).replace(/[^a-zA-Z0-9_-]/g, function (character) {
        return "\\" + character;
      });
    });
  }

  var normaliseQuery = function (query) {
    return String(query)
      .trim()
      .replace(/\s+/g, " ")
      .replace(/\s*:\s*/g, ": ")
      .replace(/\s*,\s*/g, ", ");
  };

  patch(globalThis, "matchMedia", function matchMedia(query) {
    var list = globalThis.MediaQueryList && globalThis.MediaQueryList.prototype
      ? Object.create(globalThis.MediaQueryList.prototype)
      : {};

    var fields = { matches: mediaMatches(query), media: normaliseQuery(query), onchange: null };

    env.overrides.set(list, fields);

    try {
      Object.defineProperty(list, "matches", { get: asNative(function () { return fields.matches; }, "get matches"), enumerable: true, configurable: true });
      Object.defineProperty(list, "media", { get: asNative(function () { return fields.media; }, "get media"), enumerable: true, configurable: true });
      Object.defineProperty(list, "onchange", { value: null, writable: true, enumerable: true, configurable: true });
      Object.defineProperty(list, "addListener", { value: asNative(function addListener() {}, "addListener"), writable: true, enumerable: false, configurable: true });
      Object.defineProperty(list, "removeListener", { value: asNative(function removeListener() {}, "removeListener"), writable: true, enumerable: false, configurable: true });
    } catch (error) {}

    return list;
  });

  var systemColors = snapshot.systemColors || {};
  var computedDefaults = snapshot.computedStyle || {};
  var shape = env.styleShape || null;
  var ownNames = shape && shape.own && shape.own.length ? shape.own : null;
  var ownSet = Object.create(null);

  if (ownNames) for (var o = 0; o < ownNames.length; o += 1) ownSet[ownNames[o]] = true;

  var enumerableNames = Object.create(null);
  var enumerableList = shape && Array.isArray(shape.keys) ? shape.keys : ownNames;

  if (enumerableList) for (var n = 0; n < enumerableList.length; n += 1) enumerableNames[enumerableList[n]] = true;

  var styleProto = null;

  try {
    styleProto = globalThis.CSSStyleDeclaration ? globalThis.CSSStyleDeclaration.prototype : null;
  } catch (error) {
    styleProto = null;
  }

  var computedFor = function (element) {
    var inline = element && element.style ? element.style : null;

    var resolve = function (name) {
      var property = String(name);
      var own = inline ? inline.getPropertyValue(property) : "";

      if (own) {
        if (Object.prototype.hasOwnProperty.call(systemColors, own)) return systemColors[own];
        return own;
      }

      if (Object.prototype.hasOwnProperty.call(computedDefaults, property)) return computedDefaults[property];
      var fallback = shape && (element === globalThis.document.body ? shape.body : shape.div);
      if (fallback && Object.prototype.hasOwnProperty.call(fallback, property)) return fallback[property];
      return "";
    };

    var names = shape && shape.dashed && shape.dashed.length ? shape.dashed : Object.keys(computedDefaults);

    var style = {
      getPropertyValue: asNative(function getPropertyValue(name) { return resolve(name); }, "getPropertyValue"),
      getPropertyPriority: asNative(function getPropertyPriority() { return ""; }, "getPropertyPriority"),
      item: asNative(function item(index) { return names[index] || ""; }, "item"),
      length: names.length,
    };

    var valueOfKey = function (key) {
      if (/^\d+$/.test(key)) return names[Number(key)];
      return resolve(key.replace(/[A-Z]/g, function (character) { return "-" + character.toLowerCase(); }));
    };

    return new Proxy(style, {
      get: function (target, key) {
        if (key in target) return target[key];
        if (typeof key !== "string") return undefined;
        return valueOfKey(key);
      },
      has: function (target, key) {
        return key in target || typeof key === "string";
      },
      getPrototypeOf: function () {
        return styleProto || Object.prototype;
      },
      ownKeys: function (target) {
        return ownNames ? ownNames.slice() : Object.getOwnPropertyNames(target);
      },
      getOwnPropertyDescriptor: function (target, key) {
        if (!ownNames) return Object.getOwnPropertyDescriptor(target, key);
        if (typeof key !== "string" || !ownSet[key]) return undefined;
        return { value: valueOfKey(key), writable: false, enumerable: enumerableNames[key] === true, configurable: true };
      },
    });
  };

  patch(globalThis, "getComputedStyle", function getComputedStyle(element) {
    return computedFor(element);
  });

  patch(globalThis, "getSelection", function getSelection() {
    return { toString: function () { return ""; }, rangeCount: 0 };
  });

  patch(globalThis, "scrollTo", function scrollTo() { return undefined; });
  patch(globalThis, "focus", function focus() { return undefined; });
  patch(globalThis, "blur", function blur() { return undefined; });
  patch(globalThis, "open", function open() { return null; });
  patch(globalThis, "close", function close() { return undefined; });
  patch(globalThis, "btoa", globalThis.btoa);
  patch(globalThis, "atob", globalThis.atob);

  installIterators();

  var applyShapes = function () {
    var shapes = env.shapes;
    if (!shapes) return;

    var removed = [];
    var names = Object.keys(shapes);

    for (var index = 0; index < names.length; index += 1) {
      var constructor;

      try {
        constructor = globalThis[names[index]];
      } catch (error) {
        continue;
      }

      if (typeof constructor !== "function" || !constructor.prototype) continue;

      var wanted = Object.create(null);
      for (var w = 0; w < shapes[names[index]].length; w += 1) wanted[shapes[names[index]][w]] = true;

      var own = Object.getOwnPropertyNames(constructor.prototype);

      for (var o = 0; o < own.length; o += 1) {
        if (wanted[own[o]]) continue;

        try {
          var descriptor = Object.getOwnPropertyDescriptor(constructor.prototype, own[o]);
          if (!descriptor || !descriptor.configurable) continue;
          delete constructor.prototype[own[o]];
          removed.push(names[index] + "." + own[o]);
        } catch (error) {}
      }
    }

    env.shapeRemovals = removed;
  };

  applyShapes();

  if (globalThis.Window && globalThis.Window.prototype) {
    for (var storageKind = 0; storageKind < 2; storageKind += 1) {
      try {
        Object.defineProperty(globalThis.Window.prototype, ["TEMPORARY", "PERSISTENT"][storageKind], {
          value: storageKind,
          writable: false,
          enumerable: true,
          configurable: false,
        });
      } catch (error) {}
    }
  }

  if (env.orderWindow) env.orderWindow();

  if (env.windowOrder && env.windowOrder.length) {
    var wanted = env.windowOrder;
    var rank = Object.create(null);

    for (var w = 0; w < wanted.length; w += 1) rank[wanted[w]] = w;

    var harnessName = /^__[A-Z]/;

    var inRecordedOrder = function (names) {
      var indexed = [];
      var known = [];
      var rest = [];

      for (var index = 0; index < names.length; index += 1) {
        var name = names[index];

        if (typeof name === "string" && harnessName.test(name)) continue;
        if (typeof name === "string" && /^(0|[1-9]\d*)$/.test(name)) indexed.push(name);
        else if (typeof name === "string" && rank[name] !== undefined) known.push(name);
        else rest.push(name);
      }

      indexed.sort(function (left, right) { return Number(left) - Number(right); });
      known.sort(function (left, right) { return rank[left] - rank[right]; });
      return indexed.concat(known, rest);
    };

    var realNames = Object.getOwnPropertyNames;
    var realKeys = Reflect.ownKeys;

    var defineStatic = function (holder, name, implementation) {
      try {
        var previous = Object.getOwnPropertyDescriptor(holder, name);

        Object.defineProperty(holder, name, {
          value: asNative(implementation, name),
          writable: previous ? previous.writable !== false : true,
          enumerable: previous ? previous.enumerable : false,
          configurable: true,
        });
      } catch (error) {}
    };

    defineStatic(Object, "getOwnPropertyNames", function getOwnPropertyNames(target) {
      var names = realNames(target);
      if (target !== globalThis) return names;
      env.count("getOwnPropertyNames(window)");
      return inRecordedOrder(names);
    });

    defineStatic(Reflect, "ownKeys", function ownKeys(target) {
      var names = realKeys(target);
      if (target !== globalThis) return names;
      env.count("getOwnPropertyNames(window)");
      return inRecordedOrder(names);
    });
  }

  var linkRecordedChain = function () {
    var snapshotData = env.snapshot;
    if (!snapshotData || !snapshotData.roots || !snapshotData.roots.window || snapshotData.roots.window.k !== "ref") return;

    var record = snapshotData.objects[snapshotData.roots.window.id];
    var target = globalThis;

    for (var step = 0; step < 8 && record && target; step += 1) {
      if (!record.proto || record.proto.k !== "ref") return;

      var next = env.materialize(record.proto.id, "window.[[proto]]");
      if (!next || next === target) return;

      try {
        if (Object.getPrototypeOf(target) !== next) Object.setPrototypeOf(target, next);
      } catch (error) {
        return;
      }

      target = next;
      record = snapshotData.objects[record.proto.id];
    }
  };

  if (globalThis.Node && globalThis.Node.prototype) {
    installFields(globalThis.Node.prototype, NODE_FIELDS);

    try {
      Object.defineProperty(globalThis.Node.prototype, "isConnected", {
        get: asNative(function () {
          var current = this;

          for (var step = 0; current && step < 64; step += 1) {
            if (current === globalThis.document || (env.tree && (current === env.tree.html || current === env.tree.head || current === env.tree.body))) return true;
            current = current.parentNode;
          }

          return false;
        }, "get isConnected"),
        enumerable: true,
        configurable: true,
      });
    } catch (error) {}
  }
  if (globalThis.Element && globalThis.Element.prototype) {
    installFields(globalThis.Element.prototype, ELEMENT_FIELDS);
    installMarkup(globalThis.Element.prototype);

    var reflectAttribute = function (property, attribute) {
      try {
        Object.defineProperty(globalThis.Element.prototype, property, {
          get: asNative(function () {
            var state = nodeState(this);
            if (!state) return "";
            var raw = state.attrs[attribute];
            return raw === undefined ? "" : raw;
          }, "get " + property),
          set: asNative(function (value) {
            var state = nodeState(this);
            if (!state) return;
            state.attrs[attribute] = String(value);
            state[property] = String(value);
            env.syncAttributes(this);
          }, "set " + property),
          enumerable: true,
          configurable: true,
        });
      } catch (error) {}
    };

    reflectAttribute("className", "class");
    reflectAttribute("id", "id");

    try {
      Object.defineProperty(globalThis.HTMLElement && globalThis.HTMLElement.prototype ? globalThis.HTMLElement.prototype : globalThis.Element.prototype, "dataset", {
        get: asNative(function () {
          var element = this;

          return new Proxy({}, {
            get: function (holder, key) {
              if (typeof key !== "string") return undefined;
              var attribute = "data-" + key.replace(/[A-Z]/g, function (letter) { return "-" + letter.toLowerCase(); });
              var raw = attributesOf(element)[attribute];
              return raw === undefined ? undefined : raw;
            },
            set: function (holder, key, value) {
              if (typeof key !== "string") return true;
              var attribute = "data-" + key.replace(/[A-Z]/g, function (letter) { return "-" + letter.toLowerCase(); });
              attributesOf(element)[attribute] = String(value);
              env.syncAttributes(element);
              return true;
            },
            has: function (holder, key) {
              if (typeof key !== "string") return false;
              return ("data-" + key.replace(/[A-Z]/g, function (letter) { return "-" + letter.toLowerCase(); })) in attributesOf(element);
            },
            ownKeys: function () {
              return Object.keys(attributesOf(element))
                .filter(function (name) { return name.indexOf("data-") === 0; })
                .map(function (name) { return name.slice(5).replace(/-([a-z])/g, function (all, letter) { return letter.toUpperCase(); }); });
            },
            getOwnPropertyDescriptor: function (holder, key) {
              if (typeof key !== "string") return undefined;
              var attribute = "data-" + key.replace(/[A-Z]/g, function (letter) { return "-" + letter.toLowerCase(); });
              var raw = attributesOf(element)[attribute];
              if (raw === undefined) return undefined;
              return { value: raw, writable: true, enumerable: true, configurable: true };
            },
          });
        }, "get dataset"),
        enumerable: true,
        configurable: true,
      });
    } catch (error) {}
  }
  if (globalThis.HTMLElement && globalThis.HTMLElement.prototype) installFields(globalThis.HTMLElement.prototype, HTML_FIELDS);

  linkRecordedChain();

  if (globalThis.EventTarget && globalThis.EventTarget.prototype) {
    patch(globalThis.EventTarget.prototype, "addEventListener", addListener);
    patch(globalThis.EventTarget.prototype, "removeEventListener", removeListener);
    patch(globalThis.EventTarget.prototype, "dispatchEvent", function dispatchEvent(event) {
      dispatch(this, event);
      return true;
    });
  }
})();
