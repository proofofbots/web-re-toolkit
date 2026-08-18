(function () {
  var profile = __wreProfileBlob();
  var page = __wrePageBlob();
  var hostRequest = __wreRequest;
  var hostRequestStart = __wreRequestStart;
  var hostRequestTake = __wreRequestTake;
  var hostCookieRead = __wreCookieRead;
  var hostCookieWrite = __wreCookieWrite;
  var hostCanvas = __wreCanvasImage;
  var hostMeasure = __wreMeasureText;
  var hostMiss = __wreMiss;
  var hostReal = __wreRealNow;
  var hostEntropy = __wreEntropy;
  var hostDigest = __wreDigest;
  var hostHeap = __wreHeap;

  var epoch = page.epoch;
  var realStart = hostReal();
  var skew = 0;
  var friction = page.friction || 0;
  var timers = [];
  var nextTimer = 1;
  var masked = [];

  var credited = 0;
  var floor = 0;

  var rate = 1;
  var anchor = realStart;
  var banked = 0;

  function spent() {
    return banked + (hostReal() - anchor) * rate;
  }

  function setRate(next) {
    banked = spent();
    anchor = hostReal();
    rate = Math.max(0, Number(next) || 0);

    return rate;
  }

  function elapsed() {
    var reading = spent() + skew - credited;

    if (reading < floor) return floor;
    floor = reading;
    return reading;
  }

  function credit(answer) {
    if (answer && typeof answer.paced === "number" && answer.paced > 0) credited += answer.paced;
    return answer;
  }

  function now() {
    return epoch + elapsed();
  }

  var inFlight = [];

  function startRequest(spec, deliver) {
    var ticket = hostRequestStart(spec);

    if (ticket === null || ticket === undefined) {
      deliver(credit(hostRequest(spec)));
      return;
    }

    inFlight.push({ ticket: ticket, deliver: deliver });
  }

  function collectRequest() {
    for (var index = 0; index < inFlight.length; index += 1) {
      var answer = hostRequestTake(inFlight[index].ticket);
      if (answer === null || answer === undefined) continue;

      var waiting = inFlight[index];
      inFlight.splice(index, 1);

      waiting.deliver(answer);
      return 1;
    }

    return 0;
  }

  function reach(until) {
    var short = Number(until) - elapsed();
    if (short > 0) skew += short;
    return elapsed();
  }

  function spend(weight) {
    if (friction > 0) skew += friction * (weight || 1);
  }

  function cost(ms) {
    var charge = Number(ms);
    if (charge > 0) skew += charge;
  }

  function nameAccessor(fn, prefix, property) {
    if (typeof fn !== "function") return fn;

    try {
      Object.defineProperty(fn, "name", { value: prefix + " " + property, configurable: true });
    } catch (error) {
      void error;
    }

    return fn;
  }

  function define(holder, name, getter, setter) {
    Object.defineProperty(holder, name, {
      get: nameAccessor(getter, "get", name),
      set: nameAccessor(setter, "set", name),
      enumerable: true,
      configurable: true
    });
  }

  function hide(holder, name, entry) {
    Object.defineProperty(holder, name, {
      value: entry,
      writable: true,
      enumerable: false,
      configurable: true
    });
    return entry;
  }

  function value(holder, name, entry) {
    Object.defineProperty(holder, name, {
      value: entry,
      writable: true,
      enumerable: false,
      configurable: true
    });
  }

  function tag(Ctor, name, parent) {
    Object.defineProperty(Ctor, "name", { value: name, configurable: true });
    if (parent) {
      Object.setPrototypeOf(Ctor.prototype, parent.prototype);
      Object.setPrototypeOf(Ctor, parent);
    }
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
    masked.push(name + ".prototype");
    masked.push(name);
    return Ctor;
  }

  function fnv1a(text) {
    var hash = 0x811c9dc5;
    for (var index = 0; index < text.length; index += 1) {
      hash ^= text.charCodeAt(index);
      hash = Math.imul(hash, 0x01000193) >>> 0;
    }
    return hash.toString(16);
  }

  var BASE64 = "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

  function toBase64(bytes) {
    var out = "";

    for (var index = 0; index < bytes.length; index += 3) {
      var first = bytes[index];
      var second = index + 1 < bytes.length ? bytes[index + 1] : 0;
      var third = index + 2 < bytes.length ? bytes[index + 2] : 0;
      var word = (first << 16) | (second << 8) | third;

      out += BASE64.charAt((word >> 18) & 63);
      out += BASE64.charAt((word >> 12) & 63);
      out += index + 1 < bytes.length ? BASE64.charAt((word >> 6) & 63) : "=";
      out += index + 2 < bytes.length ? BASE64.charAt(word & 63) : "=";
    }

    return out;
  }

  function fromBase64(text) {
    var clean = String(text).replace(/[^A-Za-z0-9+/]/g, "");
    var bytes = [];
    var held = 0;
    var bits = 0;

    for (var index = 0; index < clean.length; index += 1) {
      held = (held << 6) | BASE64.indexOf(clean.charAt(index));
      bits += 6;

      if (bits >= 8) {
        bits -= 8;
        bytes.push((held >> bits) & 255);
      }
    }

    return bytes;
  }

  function bytesOf(value) {
    if (value instanceof ArrayBuffer) return new Uint8Array(value);
    if (ArrayBuffer.isView(value)) {
      return new Uint8Array(value.buffer, value.byteOffset, value.byteLength);
    }
    if (value && typeof value === "object" && value.__parts) {
      var joined = value.__parts.join("");
      var out = new Uint8Array(joined.length);
      for (var index = 0; index < joined.length; index += 1) out[index] = joined.charCodeAt(index) & 255;
      return out;
    }
    return null;
  }

  var EventTargetBase = globalThis.EventTarget;

  function listeners(node) {
    if (!node.__listeners) {
      Object.defineProperty(node, "__listeners", { value: {}, enumerable: false });
    }
    return node.__listeners;
  }

  EventTargetBase.prototype.addEventListener = function (type, handler, options) {
    if (!handler) return;
    var store = listeners(this);
    var key = String(type);
    if (!store[key]) store[key] = [];
    store[key].push({ handler: handler, once: Boolean(options && options.once) });
    if (this === globalThis) armSensorEvent(key);
  };

  var armed = Object.create(null);

  function armSensorEvent(type) {
    if (type !== "deviceorientation" && type !== "devicemotion") return;
    if (armed[type]) return;

    armed[type] = true;
    globalThis.setTimeout(function () {
      fire(type, { target: "window" });
    }, 20);
  }

  EventTargetBase.prototype.removeEventListener = function (type, handler) {
    var store = listeners(this);
    var list = store[String(type)];
    if (!list) return;
    for (var index = 0; index < list.length; index += 1) {
      if (list[index].handler === handler) {
        list.splice(index, 1);
        return;
      }
    }
  };

  EventTargetBase.prototype.dispatchEvent = function (event) {
    if (!event) return true;
    var store = listeners(this);
    var list = (store[String(event.type)] || []).slice();
    var inline = this["on" + String(event.type)];

    event.currentTarget = this;
    if (!event.target) event.target = this;

    for (var index = 0; index < list.length; index += 1) {
      var entry = list[index];
      if (entry.once) this.removeEventListener(event.type, entry.handler);
      try {
        if (typeof entry.handler === "function") entry.handler.call(this, event);
        else if (entry.handler && typeof entry.handler.handleEvent === "function") {
          entry.handler.handleEvent(event);
        }
      } catch (error) {
        void error;
      }
    }

    if (typeof inline === "function") {
      try {
        inline.call(this, event);
      } catch (error) {
        void error;
      }
    }

    return !event.defaultPrevented;
  };

  masked.push("EventTarget.prototype");

  var Event = tag(function Event(type, options) {
    this.type = String(type);
    this.bubbles = Boolean(options && options.bubbles);
    this.cancelable = Boolean(options && options.cancelable);
    this.composed = Boolean(options && options.composed);
    this.isTrusted = false;
    this.defaultPrevented = false;
    this.eventPhase = 0;
    this.timeStamp = elapsed();
    this.target = null;
    this.currentTarget = null;
    this.srcElement = null;
  }, "Event");

  Event.prototype.preventDefault = function () {
    this.defaultPrevented = true;
  };
  Event.prototype.stopPropagation = function () {};
  Event.prototype.stopImmediatePropagation = function () {};
  Event.prototype.initEvent = function (type) {
    this.type = String(type);
  };
  Event.NONE = 0;
  Event.CAPTURING_PHASE = 1;
  Event.AT_TARGET = 2;
  Event.BUBBLING_PHASE = 3;

  function assign(target, options, keys, fallback) {
    for (var index = 0; index < keys.length; index += 1) {
      var key = keys[index];
      target[key] = options && options[key] !== undefined ? options[key] : fallback[key];
    }
  }

  var UIEvent = tag(function UIEvent(type, options) {
    Event.call(this, type, options);
    this.detail = (options && options.detail) || 0;
    this.view = globalThis;
  }, "UIEvent", Event);

  var MOUSE_KEYS = ["screenX", "screenY", "clientX", "clientY", "pageX", "pageY", "offsetX",
    "offsetY", "movementX", "movementY", "button", "buttons", "ctrlKey", "shiftKey", "altKey",
    "metaKey"];
  var MOUSE_DEFAULTS = {
    screenX: 0, screenY: 0, clientX: 0, clientY: 0, pageX: 0, pageY: 0, offsetX: 0, offsetY: 0,
    movementX: 0, movementY: 0, button: 0, buttons: 0, ctrlKey: false, shiftKey: false,
    altKey: false, metaKey: false
  };

  var MouseEvent = tag(function MouseEvent(type, options) {
    UIEvent.call(this, type, options);
    assign(this, options, MOUSE_KEYS, MOUSE_DEFAULTS);
    this.which = this.button + 1;
    this.relatedTarget = (options && options.relatedTarget) || null;
  }, "MouseEvent", UIEvent);

  MouseEvent.prototype.getModifierState = function () {
    return false;
  };

  var PointerEvent = tag(function PointerEvent(type, options) {
    MouseEvent.call(this, type, options);
    this.pointerId = (options && options.pointerId) || 1;
    this.width = (options && options.width) || 1;
    this.height = (options && options.height) || 1;
    this.pressure = options && options.pressure !== undefined ? options.pressure : 0;
    this.tangentialPressure = 0;
    this.tiltX = 0;
    this.tiltY = 0;
    this.twist = 0;
    this.pointerType = (options && options.pointerType) || "mouse";
    this.isPrimary = options && options.isPrimary !== undefined ? options.isPrimary : true;
  }, "PointerEvent", MouseEvent);

  PointerEvent.prototype.getCoalescedEvents = function () {
    return [];
  };
  PointerEvent.prototype.getPredictedEvents = function () {
    return [];
  };

  var WheelEvent = tag(function WheelEvent(type, options) {
    MouseEvent.call(this, type, options);
    this.deltaX = (options && options.deltaX) || 0;
    this.deltaY = (options && options.deltaY) || 0;
    this.deltaZ = 0;
    this.deltaMode = 0;
  }, "WheelEvent", MouseEvent);

  var KeyboardEvent = tag(function KeyboardEvent(type, options) {
    UIEvent.call(this, type, options);
    this.key = (options && options.key) || "";
    this.code = (options && options.code) || "";
    this.keyCode = (options && options.keyCode) || 0;
    this.charCode = (options && options.charCode) || 0;
    this.which = (options && options.which) || this.keyCode;
    this.location = 0;
    this.repeat = false;
    this.isComposing = false;
    this.ctrlKey = Boolean(options && options.ctrlKey);
    this.shiftKey = Boolean(options && options.shiftKey);
    this.altKey = Boolean(options && options.altKey);
    this.metaKey = Boolean(options && options.metaKey);
  }, "KeyboardEvent", UIEvent);

  KeyboardEvent.prototype.getModifierState = function () {
    return false;
  };

  var Touch = tag(function Touch(options) {
    var source = options || {};
    this.identifier = source.identifier || 0;
    this.target = source.target || null;
    this.clientX = source.clientX || 0;
    this.clientY = source.clientY || 0;
    this.screenX = source.screenX || 0;
    this.screenY = source.screenY || 0;
    this.pageX = source.pageX || 0;
    this.pageY = source.pageY || 0;
    this.radiusX = source.radiusX || 1;
    this.radiusY = source.radiusY || 1;
    this.rotationAngle = 0;
    this.force = source.force !== undefined ? source.force : 1;
  }, "Touch");

  var TouchList = tag(function TouchList() {
    this.length = 0;
  }, "TouchList");

  TouchList.prototype.item = function (index) {
    return this[index >>> 0] || null;
  };

  function touchList(entries) {
    var list = Object.create(TouchList.prototype);
    for (var index = 0; index < entries.length; index += 1) list[index] = entries[index];
    list.length = entries.length;
    return list;
  }

  var TouchEvent = tag(function TouchEvent(type, options) {
    UIEvent.call(this, type, options);
    var source = options || {};
    this.touches = touchList(source.touches || []);
    this.targetTouches = touchList(source.targetTouches || source.touches || []);
    this.changedTouches = touchList(source.changedTouches || source.touches || []);
    this.ctrlKey = false;
    this.shiftKey = false;
    this.altKey = false;
    this.metaKey = false;
  }, "TouchEvent", UIEvent);

  var CustomEvent = tag(function CustomEvent(type, options) {
    Event.call(this, type, options);
    this.detail = options && options.detail !== undefined ? options.detail : null;
  }, "CustomEvent", Event);

  var DeviceOrientationEvent = tag(function DeviceOrientationEvent(type, options) {
    Event.call(this, type, options);
    var source = options || {};
    this.alpha = source.alpha !== undefined ? source.alpha : null;
    this.beta = source.beta !== undefined ? source.beta : null;
    this.gamma = source.gamma !== undefined ? source.gamma : null;
    this.absolute = Boolean(source.absolute);
  }, "DeviceOrientationEvent", Event);

  DeviceOrientationEvent.requestPermission = undefined;

  var DeviceMotionEvent = tag(function DeviceMotionEvent(type, options) {
    Event.call(this, type, options);
    var source = options || {};
    this.acceleration = source.acceleration || { x: null, y: null, z: null };
    this.accelerationIncludingGravity = source.accelerationIncludingGravity
      || { x: null, y: null, z: null };
    this.rotationRate = source.rotationRate || { alpha: null, beta: null, gamma: null };
    this.interval = source.interval === undefined ? 16 : source.interval;
  }, "DeviceMotionEvent", Event);

  var ProgressEvent = tag(function ProgressEvent(type, options) {
    Event.call(this, type, options);
    this.lengthComputable = Boolean(options && options.lengthComputable);
    this.loaded = (options && options.loaded) || 0;
    this.total = (options && options.total) || 0;
  }, "ProgressEvent", Event);

  var MessageEvent = tag(function MessageEvent(type, options) {
    Event.call(this, type, options);
    this.data = options && options.data !== undefined ? options.data : null;
    this.origin = (options && options.origin) || "";
    this.source = null;
  }, "MessageEvent", Event);

  var ErrorEvent = tag(function ErrorEvent(type, options) {
    Event.call(this, type, options);
    this.message = (options && options.message) || "";
    this.filename = (options && options.filename) || "";
    this.lineno = (options && options.lineno) || 0;
    this.colno = (options && options.colno) || 0;
    this.error = (options && options.error) || null;
  }, "ErrorEvent", Event);

  var FocusEvent = tag(function FocusEvent(type, options) {
    UIEvent.call(this, type, options);
    this.relatedTarget = (options && options.relatedTarget) || null;
  }, "FocusEvent", UIEvent);

  var InputEvent = tag(function InputEvent(type, options) {
    UIEvent.call(this, type, options);
    this.data = options && options.data !== undefined ? options.data : null;
    this.inputType = (options && options.inputType) || "";
    this.isComposing = false;
    this.dataTransfer = null;
  }, "InputEvent", UIEvent);

  InputEvent.prototype.getTargetRanges = function () {
    return [];
  };

  var ClipboardEvent = tag(function ClipboardEvent(type, options) {
    Event.call(this, type, options);
    this.clipboardData = (options && options.clipboardData) || null;
  }, "ClipboardEvent", Event);

  var DOM_EXCEPTION_CODES = {
    IndexSizeError: 1, HierarchyRequestError: 3, WrongDocumentError: 4, InvalidCharacterError: 5,
    NoModificationAllowedError: 7, NotFoundError: 8, NotSupportedError: 9, InUseAttributeError: 10,
    InvalidStateError: 11, SyntaxError: 12, InvalidModificationError: 13, NamespaceError: 14,
    InvalidAccessError: 15, TypeMismatchError: 17, SecurityError: 18, NetworkError: 19,
    AbortError: 20, URLMismatchError: 21, QuotaExceededError: 22, TimeoutError: 23,
    InvalidNodeTypeError: 24, DataCloneError: 25, EncodingError: 0, NotReadableError: 0,
    UnknownError: 0, ConstraintError: 0, DataError: 0, TransactionInactiveError: 0,
    ReadOnlyError: 0, VersionError: 0, OperationError: 0, NotAllowedError: 0
  };

  var DOMException = tag(function DOMException(message, name) {
    this.message = message === undefined ? "" : String(message);
    this.name = name === undefined ? "Error" : String(name);
    this.code = DOM_EXCEPTION_CODES[this.name] || 0;
    this.stack = "";
  }, "DOMException", Error);

  DOMException.prototype.toString = function () {
    return this.message ? this.name + ": " + this.message : this.name;
  };

  var DOMRect = tag(function DOMRect(x, y, width, height) {
    this.x = x || 0;
    this.y = y || 0;
    this.width = width || 0;
    this.height = height || 0;
    this.top = this.y;
    this.left = this.x;
    this.right = this.x + this.width;
    this.bottom = this.y + this.height;
  }, "DOMRect");

  DOMRect.prototype.toJSON = function () {
    return {
      x: this.x, y: this.y, width: this.width, height: this.height,
      top: this.top, left: this.left, right: this.right, bottom: this.bottom
    };
  };

  var DOMTokenList = tag(function DOMTokenList() {
    this.length = 0;
  }, "DOMTokenList");

  function tokens(node) {
    return String(node.className || "").split(/\s+/).filter(Boolean);
  }

  DOMTokenList.prototype.add = function () {
    var list = tokens(this.__node);
    for (var index = 0; index < arguments.length; index += 1) {
      if (list.indexOf(arguments[index]) === -1) list.push(arguments[index]);
    }
    this.__node.className = list.join(" ");
  };

  DOMTokenList.prototype.remove = function () {
    var drop = Array.prototype.slice.call(arguments);
    this.__node.className = tokens(this.__node)
      .filter(function (entry) { return drop.indexOf(entry) === -1; })
      .join(" ");
  };

  DOMTokenList.prototype.contains = function (name) {
    return tokens(this.__node).indexOf(String(name)) !== -1;
  };

  DOMTokenList.prototype.toggle = function (name) {
    if (this.contains(name)) this.remove(name);
    else this.add(name);
  };

  DOMTokenList.prototype.item = function (index) {
    return tokens(this.__node)[index >>> 0] || null;
  };

  function collectionKind(name) {
    var Ctor = tag(function () {
      throw new TypeError("Illegal constructor");
    }, name);

    Ctor.prototype.item = function (index) {
      var found = this[index >>> 0];
      return found === undefined ? null : found;
    };

    Ctor.prototype.namedItem = function (name) {
      var found = this[String(name)];
      return found === undefined ? null : found;
    };

    define(Ctor.prototype, "length", function () {
      return this.__count || 0;
    });

    return Ctor;
  }

  var HTMLCollection = collectionKind("HTMLCollection");
  var HTMLAllCollection = collectionKind("HTMLAllCollection");
  var StyleSheetList = collectionKind("StyleSheetList");

  function collection(entries, Ctor) {
    var list = Object.create((Ctor || HTMLCollection).prototype);
    var items = entries || [];

    for (var index = 0; index < items.length; index += 1) {
      list[index] = items[index];

      var named = items[index] && (items[index].id || items[index].name);
      if (named && list[named] === undefined) {
        Object.defineProperty(list, named, { value: items[index], enumerable: false, configurable: true });
      }
    }

    Object.defineProperty(list, "__count", { value: items.length, enumerable: false, configurable: true });
    return list;
  }

  var CSSStyleSheet = tag(function CSSStyleSheet() {
    throw new TypeError("Illegal constructor");
  }, "CSSStyleSheet");

  CSSStyleSheet.prototype.insertRule = function insertRule() { return 0; };
  CSSStyleSheet.prototype.deleteRule = function deleteRule() {};

  function styleSheetOf(node) {
    var sheet = Object.create(CSSStyleSheet.prototype);
    sheet.type = "text/css";
    sheet.disabled = false;
    sheet.ownerNode = node;
    sheet.parentStyleSheet = null;
    sheet.title = node.getAttribute("title");
    sheet.href = node.localName === "link" ? node.href : null;
    sheet.media = { length: 0, mediaText: node.getAttribute("media") || "", item: function () { return null; } };
    sheet.cssRules = [];
    sheet.rules = sheet.cssRules;
    return sheet;
  }

  var CSSStyleDeclaration = tag(function CSSStyleDeclaration() {
    this.cssText = "";
  }, "CSSStyleDeclaration");

  CSSStyleDeclaration.prototype.getPropertyValue = function (name) {
    var key = String(name).replace(/-([a-z])/g, function (whole, letter) {
      return letter.toUpperCase();
    });
    return this[key] === undefined ? "" : String(this[key]);
  };

  CSSStyleDeclaration.prototype.setProperty = function (name, entry) {
    var key = String(name).replace(/-([a-z])/g, function (whole, letter) {
      return letter.toUpperCase();
    });
    this[key] = entry;
  };

  CSSStyleDeclaration.prototype.removeProperty = function (name) {
    var key = String(name).replace(/-([a-z])/g, function (whole, letter) {
      return letter.toUpperCase();
    });
    delete this[key];
  };

  var documentBase = "";

  var Node = tag(function Node() {
    throw new TypeError("Illegal constructor");
  }, "Node", EventTargetBase);

  var NODE_TYPES = {
    ELEMENT_NODE: 1,
    ATTRIBUTE_NODE: 2,
    TEXT_NODE: 3,
    CDATA_SECTION_NODE: 4,
    ENTITY_REFERENCE_NODE: 5,
    ENTITY_NODE: 6,
    PROCESSING_INSTRUCTION_NODE: 7,
    COMMENT_NODE: 8,
    DOCUMENT_NODE: 9,
    DOCUMENT_TYPE_NODE: 10,
    DOCUMENT_FRAGMENT_NODE: 11,
    NOTATION_NODE: 12,
    DOCUMENT_POSITION_DISCONNECTED: 1,
    DOCUMENT_POSITION_PRECEDING: 2,
    DOCUMENT_POSITION_FOLLOWING: 4,
    DOCUMENT_POSITION_CONTAINS: 8,
    DOCUMENT_POSITION_CONTAINED_BY: 16,
    DOCUMENT_POSITION_IMPLEMENTATION_SPECIFIC: 32,
  };

  Object.keys(NODE_TYPES).forEach(function (name) {
    Node[name] = NODE_TYPES[name];
    Node.prototype[name] = NODE_TYPES[name];
  });

  define(Node.prototype, "baseURI", function () {
    return String(documentBase || (globalThis.location ? location.href : ""));
  });

  function setup(node, ownerDocument, name, type) {
    node.ownerDocument = ownerDocument;
    node.nodeType = type;
    node.nodeName = type === 1 ? String(name).toUpperCase() : String(name);
    node.localName = String(name).toLowerCase();
    node.tagName = type === 1 ? node.nodeName : undefined;
    node.namespaceURI = "http://www.w3.org/1999/xhtml";
    node.childNodes = [];
    node.parentNode = null;
    node.attributes = [];
    Object.defineProperty(node, "__attributes", { value: {}, writable: true, enumerable: false, configurable: true });
    node.className = "";
    node.id = "";
    node.textContent = "";
    Object.defineProperty(node, "__style", {
      value: Object.create(CSSStyleDeclaration.prototype),
      writable: true,
      enumerable: false,
      configurable: true
    });
    node.dataset = {};
    node.hidden = false;
    node.scrollTop = 0;
    node.scrollLeft = 0;
    node.clientWidth = 0;
    node.clientHeight = 0;
    node.offsetWidth = 0;
    node.offsetHeight = 0;
    node.offsetTop = 0;
    node.offsetLeft = 0;
    node.offsetParent = null;
    return node;
  }

  var runInserted = null;
  var activationEnabled = false;

  function connected(node) {
    var walk = node;
    while (walk) {
      if (walk === document) return true;
      walk = walk.parentNode;
    }
    return false;
  }

  function activate(child) {
    if (!activationEnabled || !runInserted) return;
    if (!child || child.nodeType !== 1 || child.__activated) return;

    var name = child.localName;
    if (name !== "script" && name !== "link") return;
    if (!connected(child)) return;

    hide(child, "__activated", true);
    runInserted(child);
  }

  Node.prototype.appendChild = function (child) {
    spend(1);

    if (child.nodeType === 11) {
      var moving = child.childNodes.slice();
      child.childNodes = [];
      for (var index = 0; index < moving.length; index += 1) {
        moving[index].parentNode = null;
        this.appendChild(moving[index]);
      }
      return child;
    }

    if (child.parentNode) child.parentNode.removeChild(child);
    child.parentNode = this;
    this.childNodes.push(child);
    activate(child);
    return child;
  };

  Node.prototype.insertBefore = function (child, reference) {
    spend(1);

    if (child.nodeType === 11) {
      var carried = child.childNodes.slice();
      child.childNodes = [];
      for (var at = 0; at < carried.length; at += 1) {
        carried[at].parentNode = null;
        this.insertBefore(carried[at], reference);
      }
      return child;
    }

    var found = reference ? this.childNodes.indexOf(reference) : -1;
    if (child.parentNode) child.parentNode.removeChild(child);
    child.parentNode = this;
    if (found === -1) this.childNodes.push(child);
    else this.childNodes.splice(found, 0, child);
    activate(child);
    return child;
  };

  Node.prototype.removeChild = function (child) {
    spend(1);
    var at = this.childNodes.indexOf(child);
    if (at !== -1) this.childNodes.splice(at, 1);
    child.parentNode = null;
    return child;
  };

  Node.prototype.replaceChild = function (fresh, old) {
    var at = this.childNodes.indexOf(old);
    if (at === -1) return old;
    this.childNodes[at] = fresh;
    fresh.parentNode = this;
    old.parentNode = null;
    return old;
  };

  Node.prototype.contains = function (other) {
    var walk = other;
    while (walk) {
      if (walk === this) return true;
      walk = walk.parentNode;
    }
    return false;
  };

  Node.prototype.cloneNode = function (deep) {
    var owner = this.ownerDocument || document;
    var copy;

    if (this.nodeType === 3) return owner.createTextNode(this.data);
    if (this.nodeType === 8) return owner.createComment(this.data);
    if (this.nodeType === 11) copy = owner.createDocumentFragment();
    else {
      copy = owner.createElement(this.localName);
      for (var name in this.__attributes) {
        if (Object.prototype.hasOwnProperty.call(this.__attributes, name)) {
          copy.setAttribute(name, this.__attributes[name]);
        }
      }
      if (this.value !== undefined) copy.value = this.value;
      if (this.checked !== undefined) copy.checked = this.checked;
    }

    if (deep) {
      for (var index = 0; index < this.childNodes.length; index += 1) {
        copy.appendChild(this.childNodes[index].cloneNode(true));
      }
      hide(copy, "__parsed", this.__parsed);
    }

    return copy;
  };

  Node.prototype.hasChildNodes = function () {
    return this.childNodes.length > 0;
  };

  Node.prototype.normalize = function () {};

  Node.prototype.compareDocumentPosition = function () {
    return 0;
  };

  define(Node.prototype, "parentElement", function () {
    return this.parentNode && this.parentNode.nodeType === 1 ? this.parentNode : null;
  });

  define(Node.prototype, "firstChild", function () {
    return this.childNodes[0] || null;
  });

  define(Node.prototype, "lastChild", function () {
    return this.childNodes[this.childNodes.length - 1] || null;
  });

  define(Node.prototype, "nextSibling", function () {
    if (!this.parentNode) return null;
    var at = this.parentNode.childNodes.indexOf(this);
    return this.parentNode.childNodes[at + 1] || null;
  });

  define(Node.prototype, "previousSibling", function () {
    if (!this.parentNode) return null;
    var at = this.parentNode.childNodes.indexOf(this);
    return at > 0 ? this.parentNode.childNodes[at - 1] : null;
  });

  var Element = tag(function Element() {
    throw new TypeError("Illegal constructor");
  }, "Element", Node);

  function matchesSimple(node, selector) {
    var text = String(selector).trim();
    if (!text) return false;

    var pattern = /([#.]?[A-Za-z0-9_*-]+|\[[^\]]+\])/g;
    var part;

    while ((part = pattern.exec(text)) !== null) {
      var piece = part[0];

      if (piece.charAt(0) === "#") {
        if (node.id !== piece.slice(1)) return false;
      } else if (piece.charAt(0) === ".") {
        if (tokens(node).indexOf(piece.slice(1)) === -1) return false;
      } else if (piece.charAt(0) === "[") {
        var inner = piece.slice(1, -1);
        var split = inner.indexOf("=");
        if (split === -1) {
          if (!node.hasAttribute(inner)) return false;
        } else {
          var name = inner.slice(0, split);
          var wanted = inner.slice(split + 1).replace(/^["']|["']$/g, "");
          if (node.getAttribute(name) !== wanted) return false;
        }
      } else if (piece !== "*") {
        if (node.localName !== piece.toLowerCase()) return false;
      }
    }

    return true;
  }

  function descendants(root) {
    var out = [];
    var stack = root.childNodes.slice();

    while (stack.length) {
      var node = stack.shift();
      if (node.nodeType === 1) out.push(node);
      if (node.childNodes && node.childNodes.length) {
        stack = node.childNodes.concat(stack);
      }
    }

    return out;
  }

  function select(root, selector, first) {
    spend(2);
    var groups = String(selector).split(",");
    var pool = descendants(root);
    var out = [];

    for (var group = 0; group < groups.length; group += 1) {
      var chain = groups[group].trim().split(/\s+/);
      var last = chain[chain.length - 1];

      for (var index = 0; index < pool.length; index += 1) {
        var node = pool[index];
        if (!matchesSimple(node, last)) continue;

        var ok = true;
        var walk = node.parentNode;

        for (var step = chain.length - 2; step >= 0 && ok; step -= 1) {
          var found = false;
          while (walk) {
            if (matchesSimple(walk, chain[step])) {
              found = true;
              walk = walk.parentNode;
              break;
            }
            walk = walk.parentNode;
          }
          ok = found;
        }

        if (!ok || out.indexOf(node) !== -1) continue;
        if (first) return node;
        out.push(node);
      }
    }

    return first ? null : out;
  }

  Element.prototype.setAttribute = function (name, entry) {
    var key = String(name);
    var text = String(entry);
    this.__attributes[key] = text;

    if (this.attributes.indexOf(key) === -1) {
      var known = false;
      for (var index = 0; index < this.attributes.length; index += 1) {
        if (this.attributes[index].name === key) {
          this.attributes[index].value = text;
          known = true;
          break;
        }
      }
      if (!known) this.attributes.push({ name: key, value: text, specified: true });
    }

    if (key === "id") this.id = text;
    if (key === "class") this.className = text;
    if (key === "name" || key === "type" || key === "src" || key === "href" || key === "value") {
      if (!reflects(this, key)) this[key] = text;
    }
    if (key === "src" && this.localName === "img") beginImageLoad(this, text);
  };

  var reflectsCache = new WeakMap();

  function reflects(node, property) {
    var start = Object.getPrototypeOf(node);
    var known = reflectsCache.get(start);

    if (!known) {
      known = {};
      reflectsCache.set(start, known);
    }

    if (known[property] !== undefined) return known[property];

    var holder = start;
    var answer = false;

    while (holder) {
      var descriptor = Object.getOwnPropertyDescriptor(holder, property);
      if (descriptor) {
        answer = Boolean(descriptor.set || descriptor.get);
        break;
      }
      holder = Object.getPrototypeOf(holder);
    }

    known[property] = answer;
    return answer;
  }

  Element.prototype.getAttribute = function (name) {
    var key = String(name);
    return Object.prototype.hasOwnProperty.call(this.__attributes, key)
      ? this.__attributes[key]
      : null;
  };

  Element.prototype.hasAttribute = function (name) {
    return Object.prototype.hasOwnProperty.call(this.__attributes, String(name));
  };

  Element.prototype.removeAttribute = function (name) {
    var key = String(name);
    delete this.__attributes[key];
    this.attributes = this.attributes.filter(function (entry) { return entry.name !== key; });
  };

  Element.prototype.getAttributeNames = function () {
    return Object.keys(this.__attributes);
  };

  Element.prototype.getElementsByTagName = function (name) {
    spend(2);
    var wanted = String(name).toUpperCase();
    return descendants(this).filter(function (node) {
      return wanted === "*" || node.nodeName === wanted;
    });
  };

  Element.prototype.getElementsByClassName = function (name) {
    var wanted = String(name);
    return descendants(this).filter(function (node) {
      return tokens(node).indexOf(wanted) !== -1;
    });
  };

  Element.prototype.querySelector = function (selector) {
    return select(this, selector, true);
  };

  Element.prototype.querySelectorAll = function (selector) {
    return select(this, selector, false);
  };

  Element.prototype.matches = function (selector) {
    return matchesSimple(this, selector);
  };

  Element.prototype.closest = function (selector) {
    var walk = this;
    while (walk) {
      if (walk.nodeType === 1 && matchesSimple(walk, selector)) return walk;
      walk = walk.parentNode;
    }
    return null;
  };

  Element.prototype.getBoundingClientRect = function () {
    spend(2);
    var box = this.__box;
    if (!box) return new DOMRect(0, 0, this.offsetWidth, this.offsetHeight);
    var offset = globalThis[Symbol.for("wre.scroll")] || { x: 0, y: 0 };
    return new DOMRect(box.left - offset.x, box.top - offset.y, box.width, box.height);
  };

  Element.prototype.getClientRects = function () {
    return [this.getBoundingClientRect()];
  };

  Element.prototype.remove = function () {
    if (this.parentNode) this.parentNode.removeChild(this);
  };

  Element.prototype.focus = function () {
    ownerDocument().activeElement = this;
  };

  Element.prototype.blur = function () {
    ownerDocument().activeElement = ownerDocument().body;
  };

  Element.prototype.click = function () {
    this.dispatchEvent(new MouseEvent("click", { bubbles: true }));
  };

  Element.prototype.scrollIntoView = function () {};
  Element.prototype.insertAdjacentHTML = function () {};
  Element.prototype.setAttributeNS = function (space, name, entry) {
    this.setAttribute(name, entry);
  };
  Element.prototype.getAttributeNS = function (space, name) {
    return this.getAttribute(name);
  };

  define(Element.prototype, "style", function () {
    return this.__style;
  }, function (text) {
    this.__style.cssText = String(text);
  });

  define(Element.prototype, "classList", function () {
    if (!this.__classList) {
      var list = Object.create(DOMTokenList.prototype);
      Object.defineProperty(list, "__node", { value: this, enumerable: false });
      Object.defineProperty(this, "__classList", { value: list, enumerable: false });
    }
    return this.__classList;
  });

  define(Element.prototype, "previousElementSibling", function () {
    if (!this.parentNode) return null;
    var siblings = this.parentNode.childNodes;
    for (var at = siblings.indexOf(this) - 1; at >= 0; at -= 1) {
      if (siblings[at].nodeType === 1) return siblings[at];
    }
    return null;
  });

  define(Element.prototype, "nextElementSibling", function () {
    if (!this.parentNode) return null;
    var siblings = this.parentNode.childNodes;
    for (var at = siblings.indexOf(this) + 1; at < siblings.length; at += 1) {
      if (siblings[at].nodeType === 1) return siblings[at];
    }
    return null;
  });

  define(Element.prototype, "firstElementChild", function () {
    return this.childNodes.filter(function (node) { return node.nodeType === 1; })[0] || null;
  });

  define(Element.prototype, "lastElementChild", function () {
    var elements = this.childNodes.filter(function (node) { return node.nodeType === 1; });
    return elements[elements.length - 1] || null;
  });

  define(Element.prototype, "children", function () {
    return this.childNodes.filter(function (node) { return node.nodeType === 1; });
  });

  define(Element.prototype, "childElementCount", function () {
    return this.children.length;
  });

  var VOID_ELEMENTS = {
    area: 1, base: 1, br: 1, col: 1, embed: 1, hr: 1, img: 1, input: 1,
    link: 1, meta: 1, param: 1, source: 1, track: 1, wbr: 1,
  };

  var RAW_TEXT_ELEMENTS = { script: 1, style: 1, textarea: 1, title: 1, noscript: 1 };

  var LAYOUT_SKIP = {
    script: 1, style: 1, meta: 1, link: 1, title: 1, head: 1, base: 1, noscript: 1, template: 1
  };

  var LAYOUT_INLINE = {
    a: 1, span: 1, b: 1, i: 1, em: 1, strong: 1, small: 1, label: 1, code: 1, abbr: 1, u: 1, s: 1,
    sub: 1, sup: 1, big: 1, cite: 1, q: 1, time: 1, mark: 1
  };

  var LAYOUT_FIELD = { input: 1, select: 1, textarea: 1, button: 1 };
  var LINE_HEIGHT = 19;
  var CHAR_WIDTH = 7.2;

  var ROOT_ELEMENTS = { html: 1, head: 1, body: 1 };

  var BOOLEAN_ATTRIBUTES = { checked: 1, disabled: 1, required: 1, readonly: 1, multiple: 1, selected: 1 };

  var ATTRIBUTE_PATTERN = /([^\s=\/>]+)(?:\s*=\s*(?:"([^"]*)"|'([^']*)'|([^\s>]*)))?/g;

  function parseAttributes(node, source) {
    ATTRIBUTE_PATTERN.lastIndex = 0;
    var found;

    while ((found = ATTRIBUTE_PATTERN.exec(source))) {
      var name = found[1];
      if (!name) continue;

      var value = found[2];
      if (value === undefined) value = found[3];
      if (value === undefined) value = found[4];
      if (value === undefined) value = "";

      node.setAttribute(name, value);

      var lower = name.toLowerCase();
      if (BOOLEAN_ATTRIBUTES[lower]) node[lower === "readonly" ? "readOnly" : lower] = true;
      else if (lower === "value") { node.value = value; node.defaultValue = value; }
      else if (lower === "type" || lower === "name") node[lower] = value;
    }
  }

  function parseFragment(source, into) {
    var TAG = /<!--([\s\S]*?)-->|<(\/?)([a-zA-Z][^\s\/>]*)((?:"[^"]*"|'[^']*'|[^>])*?)(\/?)>/g;
    var stack = [into];
    var cursor = 0;
    var found;

    var roots = {};

    var top = function () { return stack[stack.length - 1]; };

    var addText = function (text) {
      if (text) top().appendChild(document.createTextNode(text));
    };

    while ((found = TAG.exec(source))) {
      addText(source.slice(cursor, found.index));
      cursor = TAG.lastIndex;

      if (found[1] !== undefined) {
        top().appendChild(document.createComment(found[1]));
        continue;
      }

      var name = found[3].toLowerCase();

      if (found[2]) {
        if (ROOT_ELEMENTS[name] && top().localName !== name) continue;

        for (var depth = stack.length - 1; depth > 0; depth -= 1) {
          if (stack[depth].localName === name) {
            stack.length = depth;
            break;
          }
        }
        continue;
      }

      if (ROOT_ELEMENTS[name] && roots[name]) {
        parseAttributes(roots[name], found[4] || "");
        continue;
      }

      var node = element(name);
      if (ROOT_ELEMENTS[name]) roots[name] = node;
      parseAttributes(node, found[4] || "");
      top().appendChild(node);

      if (VOID_ELEMENTS[name] || found[5]) continue;

      if (RAW_TEXT_ELEMENTS[name]) {
        var closing = source.toLowerCase().indexOf("</" + name, cursor);
        var end = closing === -1 ? source.length : closing;
        var body = source.slice(cursor, end);

        if (body) node.appendChild(document.createTextNode(body));
        if (name === "textarea") { node.value = body; node.defaultValue = body; }
        if (name === "script" || name === "style") node.text = body;

        cursor = closing === -1 ? source.length : source.indexOf(">", closing) + 1;
        TAG.lastIndex = cursor;
        continue;
      }

      stack.push(node);
    }

    addText(source.slice(cursor));
    return into;
  }

  function serializeChildren(node) {
    var out = "";
    for (var index = 0; index < node.childNodes.length; index += 1) {
      out += serializeNode(node.childNodes[index]);
    }
    return out;
  }

  function serializeNode(node) {
    if (node.nodeType === 3) return String(node.data === undefined ? node.textContent || "" : node.data);
    if (node.nodeType === 8) return "<!--" + String(node.data || "") + "-->";
    if (node.nodeType !== 1) return serializeChildren(node);

    var name = node.localName;
    var out = "<" + name;
    var attributes = node.__attributes || {};

    for (var key in attributes) {
      if (Object.prototype.hasOwnProperty.call(attributes, key)) {
        out += " " + key + '="' + String(attributes[key]).replace(/"/g, "&quot;") + '"';
      }
    }

    out += ">";
    if (VOID_ELEMENTS[name]) return out;
    return out + serializeChildren(node) + "</" + name + ">";
  }

  define(Element.prototype, "innerHTML", function () {
    if (this.__parsed) return serializeChildren(this);
    return this.__innerHTML || "";
  }, function (text) {
    hide(this, "__innerHTML", String(text));
    this.childNodes = [];
    hide(this, "__parsed", true);
    parseFragment(String(text), this);
  });

  define(Element.prototype, "outerHTML", function () {
    if (this.__parsed || this.childNodes.length) return serializeNode(this);
    return "<" + this.localName + ">" + (this.__innerHTML || "") + "</" + this.localName + ">";
  });

  define(Element.prototype, "innerText", function () {
    return this.textContent;
  }, function (text) {
    this.textContent = String(text);
  });

  var HTMLElement = tag(function HTMLElement() {
    throw new TypeError("Illegal constructor");
  }, "HTMLElement", Element);

  var elementKinds = {};

  function kind(name, constructorName) {
    var Ctor = typeof globalThis[constructorName] === "function"
      ? globalThis[constructorName]
      : tag(function () {
          throw new TypeError("Illegal constructor");
        }, constructorName, HTMLElement);

    elementKinds[name] = Ctor;
    return Ctor;
  }

  [
    ["p", "HTMLParagraphElement"], ["table", "HTMLTableElement"], ["tr", "HTMLTableRowElement"],
    ["td", "HTMLTableCellElement"], ["th", "HTMLTableCellElement"], ["tbody", "HTMLTableSectionElement"],
    ["thead", "HTMLTableSectionElement"], ["tfoot", "HTMLTableSectionElement"], ["caption", "HTMLTableCaptionElement"],
    ["col", "HTMLTableColElement"], ["colgroup", "HTMLTableColElement"], ["ul", "HTMLUListElement"],
    ["ol", "HTMLOListElement"], ["li", "HTMLLIElement"], ["dl", "HTMLDListElement"],
    ["h1", "HTMLHeadingElement"], ["h2", "HTMLHeadingElement"], ["h3", "HTMLHeadingElement"],
    ["h4", "HTMLHeadingElement"], ["h5", "HTMLHeadingElement"], ["h6", "HTMLHeadingElement"],
    ["br", "HTMLBRElement"], ["hr", "HTMLHRElement"], ["pre", "HTMLPreElement"],
    ["object", "HTMLObjectElement"], ["embed", "HTMLEmbedElement"], ["source", "HTMLSourceElement"],
    ["track", "HTMLTrackElement"], ["picture", "HTMLPictureElement"], ["details", "HTMLDetailsElement"],
    ["dialog", "HTMLDialogElement"], ["template", "HTMLTemplateElement"], ["slot", "HTMLSlotElement"],
    ["progress", "HTMLProgressElement"], ["meter", "HTMLMeterElement"], ["output", "HTMLOutputElement"],
    ["fieldset", "HTMLFieldSetElement"], ["legend", "HTMLLegendElement"], ["datalist", "HTMLDataListElement"],
    ["optgroup", "HTMLOptGroupElement"], ["map", "HTMLMapElement"], ["area", "HTMLAreaElement"],
    ["base", "HTMLBaseElement"], ["title", "HTMLTitleElement"], ["font", "HTMLFontElement"],
    ["marquee", "HTMLMarqueeElement"], ["frame", "HTMLFrameElement"], ["frameset", "HTMLFrameSetElement"],
    ["dir", "HTMLDirectoryElement"], ["menu", "HTMLMenuElement"], ["time", "HTMLTimeElement"],
    ["data", "HTMLDataElement"], ["blockquote", "HTMLQuoteElement"], ["q", "HTMLQuoteElement"],
    ["param", "HTMLParamElement"], ["del", "HTMLModElement"], ["ins", "HTMLModElement"],
  ].forEach(function (entry) {
    kind(entry[0], entry[1]);
  });

  kind("div", "HTMLDivElement");
  kind("span", "HTMLSpanElement");
  kind("body", "HTMLBodyElement");
  kind("head", "HTMLHeadElement");
  kind("html", "HTMLHtmlElement");
  kind("a", "HTMLAnchorElement");
  kind("img", "HTMLImageElement");
  kind("script", "HTMLScriptElement");
  var HTMLIFrameElement = kind("iframe", "HTMLIFrameElement");
  kind("style", "HTMLStyleElement");
  kind("meta", "HTMLMetaElement");
  kind("link", "HTMLLinkElement");
  kind("button", "HTMLButtonElement");
  kind("label", "HTMLLabelElement");
  kind("select", "HTMLSelectElement");
  kind("option", "HTMLOptionElement");
  kind("textarea", "HTMLTextAreaElement");
  kind("video", "HTMLVideoElement");
  kind("audio", "HTMLAudioElement");
  var HTMLInputElement = kind("input", "HTMLInputElement");
  var HTMLFormElement = kind("form", "HTMLFormElement");
  var HTMLCanvasElement = kind("canvas", "HTMLCanvasElement");
  var HTMLMediaElement = globalThis.HTMLMediaElement;

  if (HTMLMediaElement) {
    Object.setPrototypeOf(HTMLMediaElement.prototype, HTMLElement.prototype);


    elementKinds.video = tag(function () {
      throw new TypeError("Illegal constructor");
    }, "HTMLVideoElement", HTMLMediaElement);
    elementKinds.audio = tag(function () {
      throw new TypeError("Illegal constructor");
    }, "HTMLAudioElement", HTMLMediaElement);
  }

  HTMLFormElement.prototype.submit = function () {};
  HTMLFormElement.prototype.reset = function () {};

  HTMLInputElement.prototype.select = function () {};
  HTMLInputElement.prototype.setSelectionRange = function () {};

  var frameWindows = new WeakMap();

  var reflected = { src: "", srcdoc: "", name: "", sandbox: "", allow: "", loading: "eager", csp: "",
    width: "", height: "", align: "", scrolling: "", frameBorder: "", longDesc: "", marginHeight: "",
    marginWidth: "", referrerPolicy: "" };

  Object.keys(reflected).forEach(function (property) {
    var fallback = reflected[property];

    Object.defineProperty(HTMLIFrameElement.prototype, property, {
      configurable: true,
      enumerable: true,
      get: nameAccessor(function () {
        var attribute = this.getAttribute(property);
        return attribute === null || attribute === undefined ? fallback : String(attribute);
      }, "get", property),
      set: nameAccessor(function (entry) {
        this.setAttribute(property, entry);
      }, "set", property)
    });
  });

  Object.defineProperty(HTMLIFrameElement.prototype, "contentWindow", {
    configurable: true,
    enumerable: true,
    get: nameAccessor(function () {
      return frameWindows.get(this) || null;
    }, "get", "contentWindow")
  });

  Object.defineProperty(HTMLIFrameElement.prototype, "contentDocument", {
    configurable: true,
    enumerable: true,
    get: nameAccessor(function () {
      return frameWindows.has(this) ? document : null;
    }, "get", "contentDocument")
  });

  HTMLIFrameElement.prototype.getSVGDocument = function () {
    return null;
  };

  [HTMLInputElement, elementKinds.textarea].forEach(function (Ctor) {
    Object.defineProperty(Ctor.prototype, "defaultValue", {
      configurable: true,
      enumerable: false,
      get: nameAccessor(function () {
        var attribute = this.getAttribute("value");
        return attribute === null || attribute === undefined ? "" : String(attribute);
      }, "get", "defaultValue"),
      set: nameAccessor(function (entry) {
        this.setAttribute("value", entry);
      }, "set", "defaultValue")
    });
  });

  function beginImageLoad(node, reference) {
    var url = String(reference || "");
    if (!url) return;

    node.complete = false;

    var finish = function (ok, width, height) {
      node.complete = true;
      node.naturalWidth = ok ? width : 0;
      node.naturalHeight = ok ? height : 0;
      if (ok && !node.width) node.width = width;
      if (ok && !node.height) node.height = height;

      var made = new Event(ok ? "load" : "error", {});
      made.target = node;
      made.srcElement = node;
      node.dispatchEvent(made);
    };

    if (/^data:/i.test(url)) {
      globalThis.setTimeout(function () { finish(true, 1, 1); }, 1);
      return;
    }

    if (!activationEnabled) return;

    globalThis.setTimeout(function () {
      requestCount += 1;

      startRequest({
        method: "GET",
        url: resolveUrl(url, location.href),
        headers: {},
        body: null,
        at: now(),
        source: "img"
      }, function (answer) {
        finish(Boolean(answer) && answer.status >= 200 && answer.status < 300, 1, 1);
      });
    }, 1);
  }

  Object.defineProperty(elementKinds.img.prototype, "src", {
    configurable: true,
    enumerable: true,
    get: nameAccessor(function () {
      var attribute = this.getAttribute("src");
      if (attribute === null || attribute === undefined) return "";
      try {
        return resolveUrl(String(attribute), location.href);
      } catch (error) {
        return String(attribute);
      }
    }, "get", "src"),
    set: nameAccessor(function (entry) {
      this.setAttribute("src", entry);
    }, "set", "src")
  });

  function reflectUrl(Ctor, property) {
    if (!Ctor) return;

    Object.defineProperty(Ctor.prototype, property, {
      configurable: true,
      enumerable: true,
      get: nameAccessor(function () {
        var attribute = this.getAttribute(property);
        if (attribute === null || attribute === undefined || attribute === "") return "";
        try {
          return resolveUrl(String(attribute), location.href);
        } catch (error) {
          return String(attribute);
        }
      }, "get", property),
      set: nameAccessor(function (entry) {
        this.setAttribute(property, entry);
      }, "set", property)
    });
  }

  reflectUrl(elementKinds.script, "src");
  reflectUrl(elementKinds.a, "href");
  reflectUrl(elementKinds.link, "href");

  var Text = tag(function Text() {
    throw new TypeError("Illegal constructor");
  }, "Text", Node);

  var Comment = tag(function Comment() {
    throw new TypeError("Illegal constructor");
  }, "Comment", Node);

  var DocumentFragment = tag(function DocumentFragment() {
    throw new TypeError("Illegal constructor");
  }, "DocumentFragment", Node);

  var CANVAS_ENUMS = {
    DEPTH_BUFFER_BIT: 256, STENCIL_BUFFER_BIT: 1024, COLOR_BUFFER_BIT: 16384,
    POINTS: 0, LINES: 1, TRIANGLES: 4, DEPTH_TEST: 2515, BLEND: 3042, CULL_FACE: 2884,
    TEXTURE_2D: 3553, ARRAY_BUFFER: 34962, ELEMENT_ARRAY_BUFFER: 34963, STATIC_DRAW: 35044,
    FLOAT: 5126, UNSIGNED_BYTE: 5121, RGBA: 6408, VERTEX_SHADER: 35633, FRAGMENT_SHADER: 35632,
    COMPILE_STATUS: 35713, LINK_STATUS: 35714, HIGH_FLOAT: 36338, MEDIUM_FLOAT: 36337,
    LOW_FLOAT: 36336, HIGH_INT: 36341, MEDIUM_INT: 36340, LOW_INT: 36339,
    ALIASED_LINE_WIDTH_RANGE: 33902, ALIASED_POINT_SIZE_RANGE: 33901, ALPHA_BITS: 3413,
    BLUE_BITS: 3412, DEPTH_BITS: 3414, GREEN_BITS: 3411, MAX_COMBINED_TEXTURE_IMAGE_UNITS: 35661,
    MAX_CUBE_MAP_TEXTURE_SIZE: 34076, MAX_FRAGMENT_UNIFORM_VECTORS: 36349,
    MAX_RENDERBUFFER_SIZE: 34024, MAX_TEXTURE_IMAGE_UNITS: 34930, MAX_TEXTURE_SIZE: 3379,
    MAX_VARYING_VECTORS: 36348, MAX_VERTEX_ATTRIBS: 34921,
    MAX_VERTEX_TEXTURE_IMAGE_UNITS: 35660, MAX_VERTEX_UNIFORM_VECTORS: 36347,
    MAX_VIEWPORT_DIMS: 3386, RED_BITS: 3410, RENDERER: 7937, SHADING_LANGUAGE_VERSION: 35724,
    STENCIL_BITS: 3415, SUBPIXEL_BITS: 3408, VENDOR: 7936, VERSION: 7938,
    UNMASKED_VENDOR_WEBGL: 37445, UNMASKED_RENDERER_WEBGL: 37446
  };

  var CanvasRenderingContext2D = tag(function CanvasRenderingContext2D() {
    throw new TypeError("Illegal constructor");
  }, "CanvasRenderingContext2D");

  var CANVAS_STATE = ["fillStyle", "strokeStyle", "font", "textBaseline", "textAlign",
    "globalAlpha", "globalCompositeOperation", "lineWidth", "lineCap", "lineJoin", "shadowBlur",
    "shadowColor", "filter", "direction", "imageSmoothingEnabled"];

  function record(context, entry) {
    context.__ops.push(entry);
    spend(1);
  }

  function contextFor(canvas) {
    var context = Object.create(CanvasRenderingContext2D.prototype);
    context.canvas = canvas;
    context.__ops = [];
    context.fillStyle = "#000000";
    context.strokeStyle = "#000000";
    context.font = "10px sans-serif";
    context.textBaseline = "alphabetic";
    context.textAlign = "start";
    context.globalAlpha = 1;
    context.globalCompositeOperation = "source-over";
    context.lineWidth = 1;
    context.lineCap = "butt";
    context.lineJoin = "miter";
    context.shadowBlur = 0;
    context.shadowColor = "rgba(0, 0, 0, 0)";
    context.filter = "none";
    context.direction = "ltr";
    context.imageSmoothingEnabled = true;
    return context;
  }

  function drawing(context) {
    var state = [];
    for (var index = 0; index < CANVAS_STATE.length; index += 1) {
      state.push(CANVAS_STATE[index] + "=" + String(context[CANVAS_STATE[index]]));
    }
    return context.canvas.width + "x" + context.canvas.height + ";" +
      context.__ops.join(";") + ";" + state.join(";");
  }

  var CANVAS_VOIDS = ["clearRect", "fillRect", "strokeRect", "beginPath", "closePath", "moveTo",
    "lineTo", "bezierCurveTo", "quadraticCurveTo", "arc", "arcTo", "ellipse", "rect", "fill",
    "stroke", "clip", "save", "restore", "scale", "rotate", "translate", "transform",
    "setTransform", "resetTransform", "drawImage", "putImageData", "setLineDash", "strokeText"];

  CANVAS_VOIDS.forEach(function (name) {
    CanvasRenderingContext2D.prototype[name] = function () {
      record(this, name + "(" + Array.prototype.slice.call(arguments).join(",") + ")");
    };
  });

  CanvasRenderingContext2D.prototype.fillText = function (text, x, y) {
    record(this, "fillText(" + text + "," + x + "," + y + "," + this.font + "," + this.fillStyle + ")");
  };

  CanvasRenderingContext2D.prototype.measureText = function (text) {
    spend(1);
    var width = hostMeasure(this.font, String(text));
    return {
      width: width,
      actualBoundingBoxLeft: 0,
      actualBoundingBoxRight: width,
      actualBoundingBoxAscent: width > 0 ? Math.round(width * 0.09 * 100) / 100 : 0,
      actualBoundingBoxDescent: width > 0 ? Math.round(width * 0.02 * 100) / 100 : 0,
      fontBoundingBoxAscent: 0,
      fontBoundingBoxDescent: 0
    };
  };

  CanvasRenderingContext2D.prototype.getImageData = function (x, y, width, height) {
    var count = Math.max(1, Math.floor(width) * Math.floor(height) * 4);
    hostMiss("canvas getImageData(" + width + "x" + height + ")");
    return {
      data: new Uint8ClampedArray(count),
      width: width,
      height: height,
      colorSpace: "srgb"
    };
  };

  CanvasRenderingContext2D.prototype.createImageData = function (width, height) {
    return { data: new Uint8ClampedArray(Math.max(1, width * height * 4)), width: width, height: height };
  };

  CanvasRenderingContext2D.prototype.createLinearGradient = function () {
    return { addColorStop: function () {} };
  };

  CanvasRenderingContext2D.prototype.createRadialGradient = function () {
    return { addColorStop: function () {} };
  };

  CanvasRenderingContext2D.prototype.createPattern = function () {
    return null;
  };

  CanvasRenderingContext2D.prototype.isPointInPath = function () {
    return false;
  };

  CanvasRenderingContext2D.prototype.getLineDash = function () {
    return [];
  };

  var WebGLRenderingContext = globalThis.WebGLRenderingContext;

  if (WebGLRenderingContext) {
    for (var enumName in CANVAS_ENUMS) {
      if (Object.prototype.hasOwnProperty.call(CANVAS_ENUMS, enumName)) {
        WebGLRenderingContext.prototype[enumName] = CANVAS_ENUMS[enumName];
      }
    }

    WebGLRenderingContext.prototype.getContextAttributes = function () {
      return {
        alpha: true, antialias: true, depth: true, desynchronized: false,
        failIfMajorPerformanceCaveat: false, powerPreference: "default",
        premultipliedAlpha: true, preserveDrawingBuffer: false, stencil: false
      };
    };

    WebGLRenderingContext.prototype.getShaderPrecisionFormat = function () {
      return { rangeMin: 127, rangeMax: 127, precision: 23 };
    };

    var GL_VOIDS = ["clearColor", "clear", "enable", "disable", "depthFunc", "viewport",
      "bindBuffer", "bufferData", "shaderSource", "compileShader", "attachShader", "linkProgram",
      "useProgram", "vertexAttribPointer", "enableVertexAttribArray", "drawArrays", "drawElements",
      "activeTexture", "bindTexture", "texImage2D", "texParameteri", "generateMipmap",
      "deleteShader", "deleteProgram", "uniform1f", "uniform2f", "uniform3f", "uniform4f",
      "uniformMatrix4fv", "blendFunc", "pixelStorei", "readPixels", "flush", "finish"];

    GL_VOIDS.forEach(function (name) {
      WebGLRenderingContext.prototype[name] = function () {};
    });

    WebGLRenderingContext.prototype.createBuffer = function () { return {}; };
    WebGLRenderingContext.prototype.createShader = function () { return {}; };
    WebGLRenderingContext.prototype.createProgram = function () { return {}; };
    WebGLRenderingContext.prototype.createTexture = function () { return {}; };
    WebGLRenderingContext.prototype.createFramebuffer = function () { return {}; };
    WebGLRenderingContext.prototype.getAttribLocation = function () { return 0; };
    WebGLRenderingContext.prototype.getUniformLocation = function () { return {}; };
    WebGLRenderingContext.prototype.getShaderParameter = function () { return true; };
    WebGLRenderingContext.prototype.getProgramParameter = function () { return true; };
    WebGLRenderingContext.prototype.getShaderInfoLog = function () { return ""; };
    WebGLRenderingContext.prototype.getError = function () { return 0; };
    WebGLRenderingContext.prototype.isContextLost = function () { return false; };

    masked.push("WebGLRenderingContext.prototype");
  }

  HTMLCanvasElement.prototype.getContext = function (requested) {
    spend(4);
    var name = String(requested);

    if (name === "2d") {
      if (!this.__context2d) {
        Object.defineProperty(this, "__context2d", { value: contextFor(this), enumerable: false });
      }
      return this.__context2d;
    }

    if (/webgl|experimental-webgl/.test(name)) {
      var wanted = name === "webgl2" ? "__contextGl2" : "__contextGl";
      var Ctor = name === "webgl2" ? globalThis.WebGL2RenderingContext : WebGLRenderingContext;
      if (!Ctor) return null;
      if (!this[wanted]) {
        var context = Object.create(Ctor.prototype);
        context.canvas = this;
        context.drawingBufferWidth = this.width;
        context.drawingBufferHeight = this.height;
        Object.defineProperty(this, wanted, { value: context, enumerable: false });
      }
      return this[wanted];
    }

    return null;
  };

  var OffscreenCanvas = tag(function OffscreenCanvas(width, height) {
    this.width = Math.max(0, Math.floor(Number(width) || 0));
    this.height = Math.max(0, Math.floor(Number(height) || 0));
  }, "OffscreenCanvas", EventTargetBase);

  OffscreenCanvas.prototype.getContext = HTMLCanvasElement.prototype.getContext;
  OffscreenCanvas.prototype.transferToImageBitmap = function () {
    return { width: this.width, height: this.height, close: function () {} };
  };
  OffscreenCanvas.prototype.convertToBlob = function (options) {
    var type = (options && options.type) || "image/png";
    return Promise.resolve(blobOfCanvas(this, type));
  };

  HTMLCanvasElement.prototype.toDataURL = function (type) {
    spend(12);
    var ops = this.__context2d ? drawing(this.__context2d) : "empty:" + this.width + "x" + this.height;
    return hostCanvas(fnv1a(ops), String(type || "image/png"), this.width, this.height, ops);
  };

  HTMLCanvasElement.prototype.toBlob = function (callback, type) {
    if (typeof callback === "function") callback(blobOfCanvas(this, type || "image/png"));
  };

  function blobOfCanvas(canvas, type) {
    var url = HTMLCanvasElement.prototype.toDataURL.call(canvas, type);
    var comma = String(url).indexOf(",");
    var encoded = comma === -1 ? "" : String(url).slice(comma + 1);
    var bytes = "";

    try {
      bytes = globalThis.atob(encoded);
    } catch (error) {
      bytes = "";
    }

    return new globalThis.Blob([bytes], { type: type });
  }

  HTMLCanvasElement.prototype.getBoundingClientRect = function () {
    return new DOMRect(0, 0, this.width, this.height);
  };

  var Document = tag(function Document() {
    throw new TypeError("Illegal constructor");
  }, "Document", Node);

  var HTMLDocument = tag(function HTMLDocument() {
    throw new TypeError("Illegal constructor");
  }, "HTMLDocument", Document);

  Document.prototype.hasPrivateToken = function () {
    return Promise.resolve(false);
  };

  Document.prototype.hasRedemptionRecord = function () {
    return Promise.resolve(false);
  };

  Document.prototype.hasStorageAccess = function () {
    return Promise.resolve(true);
  };

  var document = Object.create(HTMLDocument.prototype);

  function ownerDocument() {
    return document;
  }

  function element(name) {
    var lower = String(name).toLowerCase();
    var Ctor = elementKinds[lower] || HTMLElement;
    var node = Object.create(Ctor.prototype);
    setup(node, document, lower, 1);

    if (lower === "canvas") {
      node.width = 300;
      node.height = 150;
    }
    if (lower === "input" || lower === "textarea" || lower === "select" || lower === "button") {
      node.value = "";
      node.type = lower === "input" ? "text" : lower === "button" ? "submit" : lower;
      node.name = "";
      node.required = false;
      node.disabled = false;
      node.checked = false;
      node.labels = [];
      node.form = null;
    }
    if (lower === "form") {
      node.elements = [];
      node.action = "";
      node.method = "get";
    }
    if (lower === "img" || lower === "script" || lower === "iframe") {
      node.src = "";
      node.complete = true;
    }
    if (lower === "iframe") {
      var frame = Object.create(globalThis);
      frame.self = frame;
      frame.window = frame;
      frame.parent = globalThis;
      frame.top = globalThis;
      frame.frameElement = node;
      frame.document = document;
      frameWindows.set(node, frame);
    }
    if (lower === "a") {
      node.href = "";
    }

    return node;
  }

  Document.prototype.createElement = function (name) {
    spend(1);
    return element(name);
  };

  Document.prototype.createElementNS = function (space, name) {
    return element(name);
  };

  Document.prototype.createTextNode = function (text) {
    var node = Object.create(Text.prototype);
    setup(node, document, "#text", 3);
    node.textContent = String(text);
    node.data = String(text);
    return node;
  };

  Document.prototype.createComment = function (text) {
    var node = Object.create(Comment.prototype);
    setup(node, document, "#comment", 8);
    node.data = String(text);
    return node;
  };

  Document.prototype.createDocumentFragment = function () {
    var node = Object.create(DocumentFragment.prototype);
    setup(node, document, "#document-fragment", 11);
    return node;
  };

  Document.prototype.createEvent = function (name) {
    var made = new Event(String(name).toLowerCase());
    made.initEvent = function (type) { this.type = String(type); };
    return made;
  };

  Document.prototype.getElementById = function (id) {
    spend(1);
    var found = descendants(document).filter(function (node) { return node.id === String(id); });
    return found[0] || null;
  };

  Document.prototype.getElementsByName = function (name) {
    return descendants(document).filter(function (node) {
      return node.getAttribute("name") === String(name);
    });
  };

  Document.prototype.getElementsByTagName = Element.prototype.getElementsByTagName;
  Document.prototype.getElementsByClassName = Element.prototype.getElementsByClassName;
  Document.prototype.querySelector = Element.prototype.querySelector;
  Document.prototype.querySelectorAll = Element.prototype.querySelectorAll;

  Document.prototype.hasFocus = function () {
    return true;
  };

  Document.prototype.write = function () {};
  Document.prototype.writeln = function () {};
  Document.prototype.open = function () {};
  Document.prototype.close = function () {};
  Document.prototype.execCommand = function () { return false; };
  Document.prototype.elementFromPoint = function (x, y) {
    var found = document.body;
    var all = document.all || [];

    for (var index = 0; index < all.length; index += 1) {
      var box = all[index].__box;
      if (!box) continue;
      if (x < box.left || x > box.left + box.width) continue;
      if (y < box.top || y > box.top + box.height) continue;
      found = all[index];
    }

    return found;
  };
  Document.prototype.createRange = function () {
    return {
      selectNodeContents: function () {},
      setStart: function () {},
      setEnd: function () {},
      getBoundingClientRect: function () { return new DOMRect(0, 0, 0, 0); }
    };
  };
  Document.prototype.createTreeWalker = function (root) {
    var pool = descendants(root);
    var at = -1;
    return {
      root: root,
      currentNode: root,
      nextNode: function () {
        at += 1;
        this.currentNode = pool[at] || null;
        return this.currentNode;
      }
    };
  };
  Document.prototype.evaluate = function () {
    return { iterateNext: function () { return null; }, snapshotLength: 0 };
  };

  define(Document.prototype, "cookie", function () {
    return hostCookieRead();
  }, function (text) {
    hostCookieWrite(String(text));
  });

  var XPathResult = tag(function XPathResult() {
    throw new TypeError("Illegal constructor");
  }, "XPathResult");

  XPathResult.ANY_TYPE = 0;
  XPathResult.NUMBER_TYPE = 1;
  XPathResult.STRING_TYPE = 2;
  XPathResult.BOOLEAN_TYPE = 3;
  XPathResult.ORDERED_NODE_SNAPSHOT_TYPE = 7;
  XPathResult.FIRST_ORDERED_NODE_TYPE = 9;

  var XPathEvaluator = tag(function XPathEvaluator() {}, "XPathEvaluator");
  XPathEvaluator.prototype.createExpression = function () {
    return { evaluate: function () { return Object.create(XPathResult.prototype); } };
  };
  XPathEvaluator.prototype.evaluate = function () {
    return Object.create(XPathResult.prototype);
  };

  var Storage = globalThis.Storage;

  if (!Storage) {
    Storage = tag(function Storage() {
      throw new TypeError("Illegal constructor");
    }, "Storage");
  } else {
    masked.push("Storage.prototype");
  }

  function storage() {
    var box = Object.create(Storage.prototype);
    Object.defineProperty(box, "__values", { value: {}, enumerable: false });
    return box;
  }

  Storage.prototype.getItem = function (key) {
    var name = String(key);
    return Object.prototype.hasOwnProperty.call(this.__values, name) ? this.__values[name] : null;
  };

  Storage.prototype.setItem = function (key, entry) {
    this.__values[String(key)] = String(entry);
  };

  Storage.prototype.removeItem = function (key) {
    delete this.__values[String(key)];
  };

  Storage.prototype.clear = function () {
    var keys = Object.keys(this.__values);
    for (var index = 0; index < keys.length; index += 1) delete this.__values[keys[index]];
  };

  Storage.prototype.key = function (index) {
    return Object.keys(this.__values)[index] || null;
  };

  define(Storage.prototype, "length", function () {
    return Object.keys(this.__values).length;
  });

  var requestCount = 0;

  var XMLHttpRequestEventTarget = tag(function XMLHttpRequestEventTarget() {
    throw new TypeError("Illegal constructor");
  }, "XMLHttpRequestEventTarget", EventTargetBase);

  var XMLHttpRequest = tag(function XMLHttpRequest() {
    this.readyState = 0;
    this.status = 0;
    this.statusText = "";
    this.responseText = "";
    this.response = "";
    this.responseType = "";
    this.responseURL = "";
    this.timeout = 0;
    this.withCredentials = false;
    this.onreadystatechange = null;
    this.onload = null;
    this.onerror = null;
    this.upload = Object.create(XMLHttpRequestEventTarget.prototype);
    Object.defineProperty(this, "__headers", { value: {}, enumerable: false, writable: true });
    Object.defineProperty(this, "__answer", { value: null, enumerable: false, writable: true });
  }, "XMLHttpRequest", XMLHttpRequestEventTarget);

  XMLHttpRequest.UNSENT = 0;
  XMLHttpRequest.OPENED = 1;
  XMLHttpRequest.HEADERS_RECEIVED = 2;
  XMLHttpRequest.LOADING = 3;
  XMLHttpRequest.DONE = 4;
  XMLHttpRequest.prototype.UNSENT = 0;
  XMLHttpRequest.prototype.OPENED = 1;
  XMLHttpRequest.prototype.DONE = 4;

  XMLHttpRequest.prototype.open = function (method, url, async) {
    this.__method = String(method);
    this.__url = resolveUrl(String(url), location.href);
    this.__headers = {};
    this.__async = async === undefined || Boolean(async);
    this.readyState = 1;
    this.dispatchEvent(new Event("readystatechange"));
  };

  XMLHttpRequest.prototype.setRequestHeader = function (name, entry) {
    this.__headers[String(name).toLowerCase()] = String(entry);
  };

  XMLHttpRequest.prototype.overrideMimeType = function () {};

  XMLHttpRequest.prototype.getAllResponseHeaders = function () {
    var answer = this.__answer;
    if (!answer || !answer.headers) return "";
    return answer.headers
      .map(function (pair) { return pair[0] + ": " + pair[1]; })
      .join("\r\n");
  };

  XMLHttpRequest.prototype.getResponseHeader = function (name) {
    var answer = this.__answer;
    if (!answer || !answer.headers) return null;
    var wanted = String(name).toLowerCase();
    for (var index = 0; index < answer.headers.length; index += 1) {
      if (String(answer.headers[index][0]).toLowerCase() === wanted) return answer.headers[index][1];
    }
    return null;
  };

  function outgoing(body) {
    if (body === undefined || body === null) return { body: null };

    var bytes = bytesOf(body);
    if (bytes) return { body: null, bodyBytes: toBase64(bytes) };

    return { body: String(body) };
  }

  XMLHttpRequest.prototype.send = function (body) {
    requestCount += 1;

    var carried = outgoing(body);

    if (carried.body !== null && carried.body !== undefined && !this.__headers["content-type"]) {
      this.__headers["content-type"] = "text/plain;charset=UTF-8";
    }

    var request = this;

    var deliver = function (answer) {
      request.__answer = answer;
      request.status = answer.status;
      request.statusText = answer.status === 200 ? "OK" : "";
      request.responseText = answer.body;
      request.response = request.responseType === "json" ? safeParse(answer.body) : answer.body;
      request.responseURL = request.__url || "";
      request.readyState = 4;

      request.dispatchEvent(new Event("readystatechange"));
      request.dispatchEvent(new ProgressEvent("load"));
      request.dispatchEvent(new ProgressEvent("loadend"));
    };

    var spec = {
      method: this.__method || "GET",
      url: this.__url || "",
      headers: this.__headers,
      body: carried.body,
      bodyBytes: carried.bodyBytes,
      at: now(),
      source: "xhr"
    };

    if (this.__async === false) {
      deliver(credit(hostRequest(spec)));
      return;
    }

    startRequest(spec, deliver);
  };

  XMLHttpRequest.prototype.abort = function () {
    this.readyState = 0;
  };

  function safeParse(text) {
    try {
      return JSON.parse(text);
    } catch (error) {
      return null;
    }
  }

  var Headers = tag(function Headers(initial) {
    Object.defineProperty(this, "__values", { value: {}, enumerable: false });
    if (initial) {
      var keys = Object.keys(initial);
      for (var index = 0; index < keys.length; index += 1) {
        this.__values[keys[index].toLowerCase()] = String(initial[keys[index]]);
      }
    }
  }, "Headers");

  Headers.prototype.get = function (name) {
    var key = String(name).toLowerCase();
    return Object.prototype.hasOwnProperty.call(this.__values, key) ? this.__values[key] : null;
  };
  Headers.prototype.set = function (name, entry) {
    this.__values[String(name).toLowerCase()] = String(entry);
  };
  Headers.prototype.has = function (name) {
    return Object.prototype.hasOwnProperty.call(this.__values, String(name).toLowerCase());
  };
  Headers.prototype.append = Headers.prototype.set;
  Headers.prototype.forEach = function (visit) {
    var keys = Object.keys(this.__values);
    for (var index = 0; index < keys.length; index += 1) visit(this.__values[keys[index]], keys[index]);
  };

  var Response = tag(function Response(body, options) {
    this.status = (options && options.status) || 200;
    this.ok = this.status >= 200 && this.status < 300;
    this.statusText = "";
    this.url = (options && options.url) || "";
    this.redirected = false;
    this.type = "basic";
    this.headers = new Headers((options && options.headers) || {});
    Object.defineProperty(this, "__body", { value: String(body || ""), enumerable: false });
  }, "Response");

  Response.prototype.text = function () {
    return Promise.resolve(this.__body);
  };
  Response.prototype.json = function () {
    return Promise.resolve(safeParse(this.__body));
  };
  Response.prototype.clone = function () {
    return this;
  };
  Response.prototype.arrayBuffer = function () {
    return Promise.resolve(new ArrayBuffer(0));
  };

  function headerPairs(headers) {
    var out = {};
    if (!headers) return out;
    if (headers instanceof Headers) {
      headers.forEach(function (entry, name) { out[name] = entry; });
      return out;
    }
    var keys = Object.keys(headers);
    for (var index = 0; index < keys.length; index += 1) {
      out[String(keys[index]).toLowerCase()] = String(headers[keys[index]]);
    }
    return out;
  }

  function fetchImpl(input, options) {
    requestCount += 1;
    var url = typeof input === "string" ? input : (input && input.url) || "";
    var settings = options || {};

    if (/^(chrome-extension|moz-extension|safari-web-extension|chrome|resource):/i.test(String(url))) {
      return Promise.reject(new TypeError("Failed to fetch"));
    }

    var carried = outgoing(settings.body);

    var answered = null;
    var settled = new Promise(function (resolve) { answered = resolve; });

    startRequest({
      method: String(settings.method || "GET"),
      url: resolveUrl(String(url), location.href),
      headers: headerPairs(settings.headers),
      body: carried.body,
      bodyBytes: carried.bodyBytes,
      at: now(),
      source: "fetch"
    }, answered);

    return settled.then(function (answer) {
      var headers = {};
      (answer.headers || []).forEach(function (pair) { headers[pair[0]] = pair[1]; });

      return new Response(answer.body, {
        status: answer.status,
        url: String(url),
        headers: headers
      });
    });
  }

  var Blob = tag(function Blob(parts, options) {
    var held = [];

    (parts || []).forEach(function (part) {
      var bytes = bytesOf(part);

      if (bytes) {
        var text = "";
        for (var index = 0; index < bytes.length; index += 1) text += String.fromCharCode(bytes[index]);
        held.push(text);
        return;
      }

      held.push(String(part));
    });

    Object.defineProperty(this, "__parts", { value: held, enumerable: false });
    this.size = held.reduce(function (total, part) { return total + part.length; }, 0);
    this.type = (options && options.type) || "";
  }, "Blob");

  Blob.prototype.text = function () {
    return Promise.resolve(this.__parts ? this.__parts.join("") : "");
  };
  Blob.prototype.slice = function () {
    return this;
  };
  Blob.prototype.arrayBuffer = function () {
    var bytes = bytesOf(this) || new Uint8Array(0);
    return Promise.resolve(bytes.buffer.slice(bytes.byteOffset, bytes.byteOffset + bytes.byteLength));
  };

  var File = tag(function File(parts, name, options) {
    Blob.call(this, parts, options);
    this.name = String(name === undefined ? "" : name);
    this.lastModified = (options && options.lastModified) || Math.floor(now());
  }, "File", Blob);

  Object.defineProperty(File.prototype, "webkitRelativePath", {
    configurable: true,
    enumerable: true,
    get: nameAccessor(function () {
      return this.__relativePath || "";
    }, "get", "webkitRelativePath")
  });

  Object.defineProperty(File.prototype, "lastModifiedDate", {
    configurable: true,
    enumerable: true,
    get: nameAccessor(function () {
      return new Date(this.lastModified);
    }, "get", "lastModifiedDate")
  });

  var FileList = tag(function FileList() {
    throw new TypeError("Illegal constructor");
  }, "FileList");

  FileList.prototype.item = function (index) {
    var found = this[index >>> 0];
    return found === undefined ? null : found;
  };

  define(FileList.prototype, "length", function () {
    return this.__count || 0;
  });

  var FormData = tag(function FormData() {
    Object.defineProperty(this, "__values", { value: [], enumerable: false });
  }, "FormData");

  FormData.prototype.append = function (name, entry) {
    this.__values.push([String(name), entry]);
  };
  FormData.prototype.get = function (name) {
    for (var index = 0; index < this.__values.length; index += 1) {
      if (this.__values[index][0] === String(name)) return this.__values[index][1];
    }
    return null;
  };
  FormData.prototype.has = function (name) {
    return this.get(name) !== null;
  };
  FormData.prototype.set = FormData.prototype.append;
  FormData.prototype.forEach = function (visit) {
    for (var index = 0; index < this.__values.length; index += 1) {
      visit(this.__values[index][1], this.__values[index][0]);
    }
  };

  var FileReader = tag(function FileReader() {
    this.readyState = 0;
    this.result = null;
    this.error = null;
    this.onload = null;
    this.onerror = null;
  }, "FileReader", EventTargetBase);

  FileReader.prototype.readAsText = function () {
    this.readyState = 2;
    this.result = "";
    if (typeof this.onload === "function") this.onload({ target: this });
  };
  FileReader.prototype.readAsDataURL = function () {
    this.readyState = 2;
    this.result = "data:,";
    if (typeof this.onload === "function") this.onload({ target: this });
  };
  FileReader.prototype.readAsArrayBuffer = function () {
    this.readyState = 2;
    this.result = new ArrayBuffer(0);
    if (typeof this.onload === "function") this.onload({ target: this });
  };
  FileReader.prototype.abort = function () {};

  var TextEncoder = tag(function TextEncoder() {
    this.encoding = "utf-8";
  }, "TextEncoder");

  TextEncoder.prototype.encode = function (input) {
    var text = String(input === undefined ? "" : input);
    var bytes = [];

    for (var index = 0; index < text.length; index += 1) {
      var code = text.charCodeAt(index);

      if (code < 0x80) {
        bytes.push(code);
      } else if (code < 0x800) {
        bytes.push(0xc0 | (code >> 6), 0x80 | (code & 0x3f));
      } else if (code >= 0xd800 && code <= 0xdbff && index + 1 < text.length) {
        var low = text.charCodeAt(index + 1);
        var point = 0x10000 + ((code - 0xd800) << 10) + (low - 0xdc00);
        index += 1;
        bytes.push(
          0xf0 | (point >> 18),
          0x80 | ((point >> 12) & 0x3f),
          0x80 | ((point >> 6) & 0x3f),
          0x80 | (point & 0x3f)
        );
      } else {
        bytes.push(0xe0 | (code >> 12), 0x80 | ((code >> 6) & 0x3f), 0x80 | (code & 0x3f));
      }
    }

    return new Uint8Array(bytes);
  };

  var TextDecoder = tag(function TextDecoder(label) {
    this.encoding = String(label || "utf-8").toLowerCase();
    this.fatal = false;
    this.ignoreBOM = false;
  }, "TextDecoder");

  TextDecoder.prototype.decode = function (input) {
    if (!input) return "";
    var bytes = input instanceof Uint8Array ? input : new Uint8Array(input.buffer || input);
    var out = "";
    var index = 0;

    while (index < bytes.length) {
      var byte = bytes[index];

      if (byte < 0x80) {
        out += String.fromCharCode(byte);
        index += 1;
      } else if (byte < 0xe0) {
        out += String.fromCharCode(((byte & 0x1f) << 6) | (bytes[index + 1] & 0x3f));
        index += 2;
      } else if (byte < 0xf0) {
        out += String.fromCharCode(
          ((byte & 0x0f) << 12) | ((bytes[index + 1] & 0x3f) << 6) | (bytes[index + 2] & 0x3f)
        );
        index += 3;
      } else {
        var point = ((byte & 0x07) << 18) | ((bytes[index + 1] & 0x3f) << 12) |
          ((bytes[index + 2] & 0x3f) << 6) | (bytes[index + 3] & 0x3f);
        point -= 0x10000;
        out += String.fromCharCode(0xd800 + (point >> 10), 0xdc00 + (point & 0x3ff));
        index += 4;
      }
    }

    return out;
  };

  var URLSearchParams = tag(function URLSearchParams(initial) {
    Object.defineProperty(this, "__values", { value: [], enumerable: false });

    if (typeof initial === "string") {
      var text = initial.charAt(0) === "?" ? initial.slice(1) : initial;
      var parts = text.length ? text.split("&") : [];

      for (var index = 0; index < parts.length; index += 1) {
        var split = parts[index].indexOf("=");
        var name = split === -1 ? parts[index] : parts[index].slice(0, split);
        var entry = split === -1 ? "" : parts[index].slice(split + 1);
        this.__values.push([decodeURIComponent(name.replace(/\+/g, " ")),
          decodeURIComponent(entry.replace(/\+/g, " "))]);
      }
    } else if (initial && typeof initial === "object") {
      var keys = Object.keys(initial);
      for (var key = 0; key < keys.length; key += 1) {
        this.__values.push([keys[key], String(initial[keys[key]])]);
      }
    }
  }, "URLSearchParams");

  URLSearchParams.prototype.get = function (name) {
    for (var index = 0; index < this.__values.length; index += 1) {
      if (this.__values[index][0] === String(name)) return this.__values[index][1];
    }
    return null;
  };
  URLSearchParams.prototype.getAll = function (name) {
    return this.__values
      .filter(function (pair) { return pair[0] === String(name); })
      .map(function (pair) { return pair[1]; });
  };
  URLSearchParams.prototype.has = function (name) {
    return this.get(name) !== null;
  };
  URLSearchParams.prototype.set = function (name, entry) {
    for (var index = 0; index < this.__values.length; index += 1) {
      if (this.__values[index][0] === String(name)) {
        this.__values[index][1] = String(entry);
        return;
      }
    }
    this.__values.push([String(name), String(entry)]);
  };
  URLSearchParams.prototype.append = function (name, entry) {
    this.__values.push([String(name), String(entry)]);
  };
  URLSearchParams.prototype.delete = function (name) {
    var values = this.__values.filter(function (pair) { return pair[0] !== String(name); });
    this.__values.length = 0;
    for (var index = 0; index < values.length; index += 1) this.__values.push(values[index]);
  };
  URLSearchParams.prototype.forEach = function (visit) {
    for (var index = 0; index < this.__values.length; index += 1) {
      visit(this.__values[index][1], this.__values[index][0]);
    }
  };
  URLSearchParams.prototype.toString = function () {
    return this.__values
      .map(function (pair) {
        return encodeURIComponent(pair[0]) + "=" + encodeURIComponent(pair[1]);
      })
      .join("&");
  };

  var URL_PATTERN = /^([a-zA-Z][a-zA-Z0-9+.-]*:)\/\/([^/?#]*)([^?#]*)(\?[^#]*)?(#.*)?$/;

  function resolveUrl(input, base) {
    var text = String(input);

    if (URL_PATTERN.test(text)) return text;
    if (!base) throw new TypeError("Invalid URL");

    var parts = URL_PATTERN.exec(String(base));
    if (!parts) throw new TypeError("Invalid base URL");

    var origin = parts[1] + "//" + parts[2];

    if (text.indexOf("//") === 0) return parts[1] + text;
    if (text.charAt(0) === "/") return origin + text;
    if (text.charAt(0) === "?") return origin + parts[3] + text;
    if (text.charAt(0) === "#") return origin + parts[3] + (parts[4] || "") + text;

    var directory = parts[3].replace(/[^/]*$/, "");
    var joined = (directory || "/") + text;
    var segments = [];

    joined.split("/").forEach(function (segment) {
      if (segment === "..") segments.pop();
      else if (segment !== "." ) segments.push(segment);
    });

    return origin + segments.join("/");
  }

  var URL = tag(function URL(input, base) {
    var text = resolveUrl(input, base);
    var parts = URL_PATTERN.exec(text);
    if (!parts) throw new TypeError("Invalid URL");

    var authority = parts[2];
    var at = authority.indexOf("@");
    var credentials = at === -1 ? "" : authority.slice(0, at);
    var hostPart = at === -1 ? authority : authority.slice(at + 1);
    var colon = hostPart.lastIndexOf(":");
    var hasPort = colon > -1 && /^\d+$/.test(hostPart.slice(colon + 1));

    this.href = text;
    this.protocol = parts[1];
    this.host = hostPart;
    this.hostname = hasPort ? hostPart.slice(0, colon) : hostPart;
    this.port = hasPort ? hostPart.slice(colon + 1) : "";
    this.pathname = parts[3] || "/";
    this.search = parts[4] || "";
    this.hash = parts[5] || "";
    this.origin = parts[1] + "//" + hostPart;
    this.username = credentials.split(":")[0] || "";
    this.password = credentials.split(":")[1] || "";
    this.searchParams = new URLSearchParams(this.search);
  }, "URL");

  URL.prototype.toString = function () {
    return this.href;
  };
  URL.prototype.toJSON = function () {
    return this.href;
  };
  var objectUrls = Object.create(null);
  var objectCount = 0;

  URL.createObjectURL = function (source) {
    var href = "blob:" + page.location.origin + "/" + fnv1a(String(now()) + String(objectCount));
    objectCount += 1;
    objectUrls[href] = source;
    return href;
  };
  URL.revokeObjectURL = function (href) {
    delete objectUrls[String(href)];
  };

  function observer(name, methods) {
    var Ctor = tag(function () {}, name);
    methods.forEach(function (method) {
      Ctor.prototype[method] = function () { return method === "takeRecords" ? [] : undefined; };
    });
    return Ctor;
  }

  observer("MutationObserver", ["observe", "disconnect", "takeRecords"]);
  observer("IntersectionObserver", ["observe", "unobserve", "disconnect", "takeRecords"]);
  observer("ResizeObserver", ["observe", "unobserve", "disconnect"]);
  observer("PerformanceObserver", ["observe", "disconnect", "takeRecords"]);
  globalThis.PerformanceObserver.supportedEntryTypes = ["mark", "measure", "navigation",
    "resource", "paint", "longtask"];

  var WebSocket = tag(function WebSocket(url) {
    this.url = String(url);
    this.readyState = 0;
    this.bufferedAmount = 0;
    this.protocol = "";
  }, "WebSocket", EventTargetBase);

  WebSocket.prototype.send = function () {};
  WebSocket.prototype.close = function () {
    this.readyState = 3;
  };
  WebSocket.CONNECTING = 0;
  WebSocket.OPEN = 1;
  WebSocket.CLOSING = 2;
  WebSocket.CLOSED = 3;

  var channels = Object.create(null);

  var BroadcastChannel = tag(function BroadcastChannel(name) {
    this.name = String(name);
    this.onmessage = null;
    this.onmessageerror = null;

    if (!channels[this.name]) channels[this.name] = [];
    channels[this.name].push(this);
  }, "BroadcastChannel", EventTargetBase);

  BroadcastChannel.prototype.postMessage = function (data) {
    var peers = channels[this.name] || [];
    var origin = this;

    peers.forEach(function (peer) {
      if (peer === origin) return;
      var event = new MessageEvent("message", { data: data, origin: page.location.origin });
      peer.dispatchEvent(event);
    });
  };

  BroadcastChannel.prototype.close = function () {
    var peers = channels[this.name] || [];
    var at = peers.indexOf(this);
    if (at !== -1) peers.splice(at, 1);
  };

  var PaymentRequest = tag(function PaymentRequest(methods) {
    this.id = fnv1a(String(now()));
    this.shippingAddress = null;
    this.shippingOption = null;
    this.shippingType = null;
    void methods;
  }, "PaymentRequest", EventTargetBase);

  PaymentRequest.prototype.canMakePayment = function () {
    return Promise.resolve(false);
  };
  PaymentRequest.prototype.show = function () {
    return Promise.reject(new DOMException("User closed the Payment Request UI.", "AbortError"));
  };
  PaymentRequest.prototype.abort = function () {
    return Promise.resolve();
  };

  var RTCPeerConnection = tag(function RTCPeerConnection() {
    this.iceConnectionState = "new";
    this.connectionState = "new";
    this.localDescription = null;
  }, "RTCPeerConnection", EventTargetBase);

  RTCPeerConnection.prototype.createDataChannel = function () {
    return { close: function () {}, send: function () {} };
  };
  RTCPeerConnection.prototype.createOffer = function () {
    return Promise.resolve({ type: "offer", sdp: "" });
  };
  RTCPeerConnection.prototype.setLocalDescription = function () {
    return Promise.resolve();
  };
  RTCPeerConnection.prototype.setRemoteDescription = function () {
    return Promise.resolve();
  };
  RTCPeerConnection.prototype.close = function () {};

  var WORKER_NAVIGATOR = ["userAgent", "appVersion", "appName", "appCodeName", "product",
    "platform", "language", "languages", "onLine", "hardwareConcurrency", "deviceMemory",
    "userAgentData", "connection", "webdriver", "storage", "permissions", "vendor", "vendorSub",
    "productSub", "maxTouchPoints"];

  function workerNavigator() {
    var carried = Object.create(null);

    WORKER_NAVIGATOR.forEach(function (name) {
      if (name in globalThis.navigator) carried[name] = globalThis.navigator[name];
    });

    return carried;
  }

  function workerScope(name) {
    var scope = {
      name: String(name || ""),
      navigator: workerNavigator(),
      location: globalThis.location,
      performance: globalThis.performance,
      fetch: globalThis.fetch,
      OffscreenCanvas: globalThis.OffscreenCanvas,
      XMLHttpRequest: globalThis.XMLHttpRequest,
      crypto: globalThis.crypto,
      caches: undefined,
      indexedDB: globalThis.indexedDB,
      onconnect: undefined,
      onmessage: undefined,
      onerror: undefined,
      document: undefined,
      window: undefined,
      parent: undefined,
      top: undefined,
      frames: undefined,
      screen: undefined,
      history: undefined,
      localStorage: undefined,
      sessionStorage: undefined,
      importScripts: function () {},
      close: function () {},
      postMessage: function () {},
      addEventListener: function (kind, handler) {
        if (typeof handler !== "function") return;
        var slot = "on" + String(kind);
        var held = scope[slot];

        scope[slot] = typeof held === "function"
          ? function (event) { held.call(this, event); handler.call(this, event); }
          : handler;
      },
      removeEventListener: function () {},
      dispatchEvent: function () { return true; }
    };

    scope.self = scope;
    scope.globalThis = scope;
    return scope;
  }

  function sourceOf(url) {
    var held = objectUrls[String(url)];
    if (!held) return null;
    if (typeof held === "string") return held;
    if (held.__parts) return held.__parts.join("");
    return null;
  }

  function runWorker(source, scope) {
    var body = new Function("self", "with (self) { " + source + "\n}");
    body.call(scope, scope);
  }

  function messagePort(target) {
    var port = {
      onmessageerror: null,
      close: function () {},
      removeEventListener: function () {},
      postMessage: function (data) {
        target(data);
      },
      __handlers: [],
      __queue: [],
      __listening: false
    };

    port.start = function () {
      port.__listening = true;
      flushPort(port);
    };

    port.addEventListener = function (kind, handler) {
      if (kind !== "message" || typeof handler !== "function") return;
      port.__handlers.push(handler);
      port.__listening = true;
      flushPort(port);
    };

    Object.defineProperty(port, "onmessage", {
      configurable: true,
      enumerable: true,
      get: function () {
        return port.__onmessage || null;
      },
      set: function (handler) {
        port.__onmessage = typeof handler === "function" ? handler : null;
        if (port.__onmessage) {
          port.__listening = true;
          flushPort(port);
        }
      }
    });

    return port;
  }

  function flushPort(port) {
    if (!port.__listening) return;

    while (port.__queue.length) {
      var event = port.__queue.shift();
      if (typeof port.__onmessage === "function") port.__onmessage(event);
      port.__handlers.forEach(function (handler) { handler(event); });
    }
  }

  function deliver(port, data) {
    port.__queue.push({ data: data, type: "message", ports: [] });
    flushPort(port);
  }

  var Worker = tag(function Worker(url) {
    var scope = workerScope(url);
    var host = this;

    Object.defineProperty(this, "__scope", { value: scope, enumerable: false });

    this.onmessage = null;
    this.onerror = null;

    var source = sourceOf(url);
    if (!source) {
      hostMiss("worker source for " + String(url));
      return;
    }

    scope.postMessage = function (data) {
      var event = { data: data, type: "message", ports: [] };
      if (typeof host.onmessage === "function") host.onmessage(event);
    };

    try {
      runWorker(source, scope);
    } catch (error) {
      hostMiss("worker threw " + (error && error.message));
    }
  }, "Worker", EventTargetBase);

  Worker.prototype.postMessage = function (data) {
    var scope = this.__scope;
    if (!scope) return;
    var event = { data: data, type: "message", ports: [] };
    if (typeof scope.onmessage === "function") scope.onmessage(event);
  };
  Worker.prototype.terminate = function () {};

  var SharedWorker = tag(function SharedWorker(url, options) {
    cost(3 + Math.random() * 26);

    var scope = workerScope(options && options.name);
    var host = this;

    Object.defineProperty(this, "__scope", { value: scope, enumerable: false });

    var inner = null;
    this.port = messagePort(function (data) {
      if (inner) deliver(inner, data);
    });
    this.onerror = null;

    var source = sourceOf(url);
    if (!source) {
      hostMiss("shared worker source for " + String(url));
      return;
    }

    try {
      runWorker(source, scope);
    } catch (error) {
      hostMiss("shared worker threw " + (error && error.message));
      return;
    }

    if (typeof scope.onconnect !== "function") return;

    inner = messagePort(function (data) { deliver(host.port, data); });

    try {
      scope.onconnect({ type: "connect", ports: [inner], data: null });
    } catch (error) {
      hostMiss("shared worker connect threw " + (error && error.message));
    }
  }, "SharedWorker", EventTargetBase);

  var Credential = tag(function Credential() {
    throw new TypeError("Illegal constructor");
  }, "Credential");

  Credential.isConditionalMediationAvailable = function isConditionalMediationAvailable() {
    return Promise.resolve(false);
  };

  define(Credential.prototype, "id", function () { return this.__id || ""; });
  define(Credential.prototype, "type", function () { return this.__type || ""; });

  var PublicKeyCredential = tag(function PublicKeyCredential() {
    throw new TypeError("Illegal constructor");
  }, "PublicKeyCredential", Credential);

  [
    "getClientCapabilities", "isConditionalMediationAvailable",
    "isUserVerifyingPlatformAuthenticatorAvailable"
  ].forEach(function (name) {
    PublicKeyCredential[name] = function () {
      return Promise.resolve(name === "getClientCapabilities" ? {} : false);
    };
    Object.defineProperty(PublicKeyCredential[name], "name", { value: name, configurable: true });
  });

  ["parseCreationOptionsFromJSON", "parseRequestOptionsFromJSON"].forEach(function (name) {
    PublicKeyCredential[name] = function (options) { return options; };
    Object.defineProperty(PublicKeyCredential[name], "name", { value: name, configurable: true });
  });

  ["signalAllAcceptedCredentials", "signalCurrentUserDetails", "signalUnknownCredential"].forEach(function (name) {
    PublicKeyCredential[name] = function () { return Promise.resolve(); };
    Object.defineProperty(PublicKeyCredential[name], "name", { value: name, configurable: true });
  });

  define(PublicKeyCredential.prototype, "rawId", function () { return null; });
  define(PublicKeyCredential.prototype, "response", function () { return null; });
  define(PublicKeyCredential.prototype, "authenticatorAttachment", function () { return null; });
  PublicKeyCredential.prototype.getClientExtensionResults = function getClientExtensionResults() {
    return {};
  };
  PublicKeyCredential.prototype.toJSON = function toJSON() {
    return {};
  };

  var AuthenticatorResponse = tag(function AuthenticatorResponse() {
    throw new TypeError("Illegal constructor");
  }, "AuthenticatorResponse");

  define(AuthenticatorResponse.prototype, "clientDataJSON", function () { return null; });

  var AuthenticatorAttestationResponse = tag(function AuthenticatorAttestationResponse() {
    throw new TypeError("Illegal constructor");
  }, "AuthenticatorAttestationResponse", AuthenticatorResponse);

  define(AuthenticatorAttestationResponse.prototype, "attestationObject", function () { return null; });
  ["getAuthenticatorData", "getPublicKey", "getPublicKeyAlgorithm", "getTransports"].forEach(function (name) {
    AuthenticatorAttestationResponse.prototype[name] = function () { return null; };
    Object.defineProperty(AuthenticatorAttestationResponse.prototype[name], "name", { value: name, configurable: true });
  });

  var AuthenticatorAssertionResponse = tag(function AuthenticatorAssertionResponse() {
    throw new TypeError("Illegal constructor");
  }, "AuthenticatorAssertionResponse", AuthenticatorResponse);

  define(AuthenticatorAssertionResponse.prototype, "authenticatorData", function () { return null; });
  define(AuthenticatorAssertionResponse.prototype, "signature", function () { return null; });
  define(AuthenticatorAssertionResponse.prototype, "userHandle", function () { return null; });

  var MediaMetadata = tag(function MediaMetadata(options) {
    var source = options || {};
    this.title = String(source.title === undefined ? "" : source.title);
    this.artist = String(source.artist === undefined ? "" : source.artist);
    this.album = String(source.album === undefined ? "" : source.album);
    this.artwork = source.artwork || [];
    this.chapterInfo = source.chapterInfo || [];
  }, "MediaMetadata");

  var MediaSession = tag(function MediaSession() {
    throw new TypeError("Illegal constructor");
  }, "MediaSession");

  MediaSession.prototype.setActionHandler = function setActionHandler() {};
  MediaSession.prototype.setCameraActive = function setCameraActive() {};
  MediaSession.prototype.setMicrophoneActive = function setMicrophoneActive() {};
  MediaSession.prototype.setPositionState = function setPositionState() {};

  var mediaSession = Object.create(MediaSession.prototype);
  mediaSession.metadata = null;
  mediaSession.playbackState = "none";

  var PushManager = tag(function PushManager() {
    throw new TypeError("Illegal constructor");
  }, "PushManager");

  PushManager.prototype.getSubscription = function () {
    return Promise.resolve(null);
  };
  PushManager.prototype.subscribe = function () {
    return Promise.reject(new Error("not supported"));
  };
  PushManager.supportedContentEncodings = ["aes128gcm"];

  var Notification = tag(function Notification() {}, "Notification", EventTargetBase);
  Notification.permission = profile.permissions && profile.permissions.notifications
    ? profile.permissions.notifications
    : "default";
  Notification.requestPermission = function () {
    return Promise.resolve(Notification.permission);
  };

  var audioSettings = profile.audio || {};

  var AudioContext = tag(function AudioContext() {
    this.sampleRate = audioSettings.sample_rate || 44100;
    this.state = audioSettings.state || "suspended";
    this.baseLatency = audioSettings.base_latency || 0;
    this.outputLatency = audioSettings.output_latency || 0;
    this.currentTime = 0;
    this.destination = {
      channelCount: audioSettings.channel_count || 2,
      maxChannelCount: audioSettings.max_channel_count || 2,
      connect: function () {},
      disconnect: function () {}
    };
    this.listener = {};
    this.audioWorklet = audioWorklet;
  }, "AudioContext", EventTargetBase);

  var AudioWorklet = tag(function AudioWorklet() {
    throw new TypeError("Illegal constructor");
  }, "AudioWorklet", EventTargetBase);

  AudioWorklet.prototype.addModule = function () {
    return Promise.resolve();
  };

  var audioWorklet = Object.create(AudioWorklet.prototype);

  function audioNode(extra) {
    var node = {
      connect: function () { return node; },
      disconnect: function () {},
      start: function () {},
      stop: function () {}
    };
    if (extra) {
      var keys = Object.keys(extra);
      for (var index = 0; index < keys.length; index += 1) node[keys[index]] = extra[keys[index]];
    }
    return node;
  }

  function param(initial) {
    return { value: initial, setValueAtTime: function () {}, linearRampToValueAtTime: function () {} };
  }

  AudioContext.prototype.createOscillator = function () {
    return audioNode({ type: "sine", frequency: param(440), detune: param(0) });
  };

  AudioContext.prototype.createAnalyser = function () {
    var node = audioNode({
      fftSize: 2048,
      minDecibels: -100,
      maxDecibels: -30,
      smoothingTimeConstant: 0.8,
      getFloatFrequencyData: function (target) {
        for (var index = 0; index < target.length; index += 1) target[index] = -Infinity;
      },
      getByteFrequencyData: function (target) {
        for (var index = 0; index < target.length; index += 1) target[index] = 0;
      },
      getFloatTimeDomainData: function (target) {
        for (var index = 0; index < target.length; index += 1) target[index] = 0;
      },
      getByteTimeDomainData: function (target) {
        for (var index = 0; index < target.length; index += 1) target[index] = 128;
      }
    });

    Object.defineProperty(node, "frequencyBinCount", {
      configurable: true,
      enumerable: true,
      get: function () { return node.fftSize / 2; }
    });

    return node;
  };

  AudioContext.prototype.decodeAudioData = function (buffer, onSuccess, onError) {
    spend(2);
    cost(6);

    var bytes = bytesOf(buffer);

    if (!bytes || bytes.length < 32) {
      var failure = new DOMException("Unable to decode audio data", "EncodingError");
      if (typeof onError === "function") onError(failure);
      return Promise.reject(failure);
    }

    var decoded = this.createBuffer(2, bytes.length, this.sampleRate);
    if (typeof onSuccess === "function") onSuccess(decoded);
    return Promise.resolve(decoded);
  };
  AudioContext.prototype.createGain = function () {
    return audioNode({ gain: param(1) });
  };
  AudioContext.prototype.createDynamicsCompressor = function () {
    return audioNode({
      threshold: param(-24), knee: param(30), ratio: param(12),
      attack: param(0.003), release: param(0.25),
      reduction: audioSettings.reduction === undefined ? 0 : audioSettings.reduction
    });
  };
  AudioContext.prototype.createBufferSource = function () {
    return audioNode({ buffer: null, loop: false });
  };
  AudioContext.prototype.createBuffer = function (channels, length, rate) {
    return {
      numberOfChannels: channels,
      length: length,
      sampleRate: rate,
      getChannelData: function () { return new Float32Array(length); }
    };
  };
  AudioContext.prototype.createScriptProcessor = function () {
    return audioNode({ onaudioprocess: null });
  };
  AudioContext.prototype.close = function () {
    this.state = "closed";
    return Promise.resolve();
  };
  AudioContext.prototype.resume = function () {
    this.state = "running";
    return Promise.resolve();
  };
  AudioContext.prototype.suspend = function () {
    return Promise.resolve();
  };

  var OfflineAudioContext = tag(function OfflineAudioContext(channels, length, rate) {
    AudioContext.call(this);
    this.length = length || 0;
    this.sampleRate = rate || this.sampleRate;
  }, "OfflineAudioContext", AudioContext);

  OfflineAudioContext.prototype.startRendering = function () {
    var length = this.length;
    var rendered = audioSettings.rendered || "";

    cost(Math.min(250, length / 441));

    return Promise.resolve({
      numberOfChannels: 1,
      length: length,
      sampleRate: this.sampleRate,
      getChannelData: function () {
        var data = new Float32Array(length);
        if (!rendered) {
          hostMiss("OfflineAudioContext rendering");
          return data;
        }

        var seed = parseInt(rendered.replace(/^[a-z0-9]+:/i, "").slice(0, 8), 16) || 1;
        var total = 0;

        for (var index = 0; index < length; index += 1) {
          seed = (Math.imul(seed, 1664525) + 1013904223) >>> 0;
          data[index] = (seed / 4294967296) * 0.0002;
          total += data[index];
        }

        var wanted = audioSettings.rendered_sum;
        if (typeof wanted === "number" && total !== 0) {
          var scale = wanted / total;
          for (var at = 0; at < length; at += 1) data[at] *= scale;
        }

        return data;
      }
    });
  };

  var SpeechSynthesisVoice = tag(function SpeechSynthesisVoice() {
    throw new TypeError("Illegal constructor");
  }, "SpeechSynthesisVoice");

  var voices = (profile.voices || []).map(function (entry) {
    var voice = Object.create(SpeechSynthesisVoice.prototype);
    voice.name = entry.name;
    voice.lang = entry.lang;
    voice.default = Boolean(entry.default);
    voice.localService = Boolean(entry.local_service);
    voice.voiceURI = entry.voice_uri || entry.name;
    return voice;
  });

  var SpeechSynthesis = tag(function SpeechSynthesis() {
    throw new TypeError("Illegal constructor");
  }, "SpeechSynthesis", EventTargetBase);

  SpeechSynthesis.prototype.getVoices = function () {
    if (!voices.length) hostMiss("speechSynthesis.getVoices");
    return voices.slice();
  };
  SpeechSynthesis.prototype.speak = function () {};
  SpeechSynthesis.prototype.cancel = function () {};
  SpeechSynthesis.prototype.pause = function () {};
  SpeechSynthesis.prototype.resume = function () {};

  var speechSynthesis = Object.create(SpeechSynthesis.prototype);
  speechSynthesis.pending = false;
  speechSynthesis.speaking = false;
  speechSynthesis.paused = false;

  var Image = tag(function Image(width, height) {
    var node = element("img");
    node.width = width || 0;
    node.height = height || 0;
    node.naturalWidth = node.width;
    node.naturalHeight = node.height;
    return node;
  }, "Image");

  var Location = tag(function Location() {
    throw new TypeError("Illegal constructor");
  }, "Location");

  Location.prototype.assign = function () {};
  Location.prototype.replace = function () {};
  Location.prototype.reload = function () {};
  Location.prototype.toString = function () {
    return this.href;
  };

  var History = tag(function History() {
    throw new TypeError("Illegal constructor");
  }, "History");

  History.prototype.pushState = function () {};
  History.prototype.replaceState = function () {};
  History.prototype.go = function () {};
  History.prototype.back = function () {};
  History.prototype.forward = function () {};

  var Crypto = tag(function Crypto() {
    throw new TypeError("Illegal constructor");
  }, "Crypto");

  function entropy(count) {
    return fromBase64(hostEntropy(count));
  }

  Crypto.prototype.getRandomValues = function (target) {
    if (!ArrayBuffer.isView(target)) {
      throw new TypeError(
        "Failed to execute 'getRandomValues' on 'Crypto': parameter 1 is not of type 'ArrayBufferView'."
      );
    }

    if (target.byteLength > 65536) {
      throw new DOMException(
        "The ArrayBufferView's byte length (" + target.byteLength + ") exceeds the number of bytes of entropy available via this API (65536).",
        "QuotaExceededError"
      );
    }

    var bytes = entropy(target.byteLength);
    var view = new Uint8Array(target.buffer, target.byteOffset, target.byteLength);
    for (var index = 0; index < view.length; index += 1) view[index] = bytes[index];

    return target;
  };

  Crypto.prototype.randomUUID = function () {
    var bytes = entropy(16);
    bytes[6] = (bytes[6] & 0x0f) | 0x40;
    bytes[8] = (bytes[8] & 0x3f) | 0x80;

    var hex = [];
    for (var index = 0; index < 16; index += 1) {
      hex.push((bytes[index] + 0x100).toString(16).slice(1));
    }

    return [
      hex.slice(0, 4).join(""),
      hex.slice(4, 6).join(""),
      hex.slice(6, 8).join(""),
      hex.slice(8, 10).join(""),
      hex.slice(10, 16).join("")
    ].join("-");
  };

  var SubtleCrypto = tag(function SubtleCrypto() {
    throw new TypeError("Illegal constructor");
  }, "SubtleCrypto");

  SubtleCrypto.prototype.digest = function (algorithm, data) {
    var name = typeof algorithm === "string" ? algorithm : (algorithm && algorithm.name) || "";
    var bytes = bytesOf(data);

    if (!bytes) {
      return Promise.reject(
        new TypeError("Failed to execute 'digest' on 'SubtleCrypto': parameter 2 is not of type 'BufferSource'.")
      );
    }

    try {
      var out = fromBase64(hostDigest(String(name), toBase64(bytes)));
      return Promise.resolve(new Uint8Array(out).buffer);
    } catch (error) {
      return Promise.reject(new DOMException("Unrecognized algorithm name", "NotSupportedError"));
    }
  };

  var subtle = Object.create(SubtleCrypto.prototype);

  define(Crypto.prototype, "subtle", function () {
    return subtle;
  });

  var VisualViewport = tag(function VisualViewport() {
    throw new TypeError("Illegal constructor");
  }, "VisualViewport", EventTargetBase);

  var timerControl = {
    add: function (fn, delay, args, repeating) {
      var id = nextTimer;
      nextTimer += 1;
      timers.push({
        id: id,
        fn: fn,
        due: elapsed() + Math.max(0, Number(delay) || 0),
        every: repeating ? Math.max(1, Number(delay) || 1) : 0,
        args: args || [],
        cancelled: false
      });
      return id;
    },
    cancel: function (id) {
      for (var index = 0; index < timers.length; index += 1) {
        if (timers[index].id === id) timers[index].cancelled = true;
      }
    }
  };

  globalThis.setTimeout = function (fn, delay) {
    return timerControl.add(fn, delay, Array.prototype.slice.call(arguments, 2), false);
  };
  globalThis.setInterval = function (fn, delay) {
    return timerControl.add(fn, delay, Array.prototype.slice.call(arguments, 2), true);
  };
  globalThis.clearTimeout = function (id) {
    timerControl.cancel(id);
  };
  globalThis.clearInterval = globalThis.clearTimeout;
  globalThis.requestAnimationFrame = function (fn) {
    return timerControl.add(function () { fn(elapsed()); }, 16, [], false);
  };
  globalThis.cancelAnimationFrame = globalThis.clearTimeout;
  globalThis.requestIdleCallback = function (fn) {
    return timerControl.add(function () {
      fn({ didTimeout: false, timeRemaining: function () { return 50; } });
    }, 1, [], false);
  };
  globalThis.cancelIdleCallback = globalThis.clearTimeout;
  globalThis.queueMicrotask = function (fn) {
    Promise.resolve().then(fn);
  };

  function runOne(until) {
    if (collectRequest()) return 1;

    var next = null;

    for (var index = 0; index < timers.length; index += 1) {
      var timer = timers[index];
      if (timer.cancelled || timer.due > until) continue;
      if (!next || timer.due < next.due || (timer.due === next.due && timer.id < next.id)) {
        next = timer;
      }
    }

    if (!next) {
      timers = timers.filter(function (timer) { return !timer.cancelled; });
      return 0;
    }

    reach(next.due);

    if (next.every > 0) next.due = elapsed() + next.every;
    else next.cancelled = true;

    try {
      next.fn.apply(globalThis, next.args);
    } catch (error) {
      void error;
    }

    return 1;
  }

  function runDue(until) {
    var ran = 0;

    for (var guard = 0; guard < 50000; guard += 1) {
      if (!runOne(until)) break;
      ran += 1;
    }

    timers = timers.filter(function (timer) { return !timer.cancelled; });
    return ran;
  }

  var RealDate = Date;

  function SandboxDate(a, b, c, d, e, f, g) {
    if (!(this instanceof SandboxDate)) return new RealDate(now()).toString();
    if (arguments.length === 0) return new RealDate(now());
    if (arguments.length === 1) return new RealDate(a);
    return new RealDate(a, b, arguments.length > 2 ? c : 1, arguments.length > 3 ? d : 0,
      arguments.length > 4 ? e : 0, arguments.length > 5 ? f : 0, arguments.length > 6 ? g : 0);
  }

  SandboxDate.prototype = RealDate.prototype;
  SandboxDate.now = function () {
    return Math.floor(now());
  };
  SandboxDate.parse = RealDate.parse;
  SandboxDate.UTC = RealDate.UTC;
  Object.defineProperty(SandboxDate, "name", { value: "Date", configurable: true });
  Object.defineProperty(SandboxDate, "length", { value: 7, configurable: true });
  globalThis.Date = SandboxDate;
  masked.push("Date");
  masked.push("Date.now");

  var timeOrigin = epoch;

  var Performance = tag(function Performance() {
    throw new TypeError("Illegal constructor");
  }, "Performance", EventTargetBase);

  Performance.prototype.now = function () {
    return elapsed();
  };
  Performance.prototype.getEntries = function () { return []; };
  Performance.prototype.getEntriesByType = function (name) {
    if (name === "navigation") {
      return [{
        name: page.location.href,
        entryType: "navigation",
        startTime: 0,
        duration: 0,
        type: "navigate",
        redirectCount: 0
      }];
    }
    return [];
  };
  Performance.prototype.getEntriesByName = function () { return []; };
  Performance.prototype.mark = function () {};
  Performance.prototype.measure = function () {};
  Performance.prototype.clearMarks = function () {};
  Performance.prototype.clearMeasures = function () {};
  Performance.prototype.clearResourceTimings = function () {};
  Performance.prototype.setResourceTimingBufferSize = function () {};
  Performance.prototype.toJSON = function () {
    return { timeOrigin: timeOrigin };
  };

  var performance = Object.create(Performance.prototype);
  define(performance, "timeOrigin", function () { return timeOrigin; });
  define(performance, "timing", function () {
    var start = Math.floor(epoch);
    return {
      navigationStart: start,
      unloadEventStart: 0,
      unloadEventEnd: 0,
      redirectStart: 0,
      redirectEnd: 0,
      fetchStart: start + 2,
      domainLookupStart: start + 3,
      domainLookupEnd: start + 8,
      connectStart: start + 8,
      connectEnd: start + 30,
      secureConnectionStart: start + 14,
      requestStart: start + 31,
      responseStart: start + 90,
      responseEnd: start + 120,
      domLoading: start + 95,
      domInteractive: start + 190,
      domContentLoadedEventStart: start + 191,
      domContentLoadedEventEnd: start + 193,
      domComplete: start + 260,
      loadEventStart: start + 261,
      loadEventEnd: start + 262,
      toJSON: function () { return this; }
    };
  });
  define(performance, "navigation", function () {
    return { type: 0, redirectCount: 0, TYPE_NAVIGATE: 0, TYPE_RELOAD: 1, toJSON: function () { return this; } };
  });

  var heapOffset = null;

  if (profile.memory) {
    define(performance, "memory", function () {
      var live = hostHeap();
      var total = Math.max(profile.memory.total_js_heap_size, live[0] || 0);
      var used = Math.max(profile.memory.used_js_heap_size, live[1] || 0);

      if (heapOffset === null) {
        heapOffset = [Math.floor(Math.random() * 4093) + 1, Math.floor(Math.random() * 4093) + 1];
      }

      total += heapOffset[0];
      used += heapOffset[1];

      return {
        jsHeapSizeLimit: profile.memory.js_heap_size_limit,
        totalJSHeapSize: total,
        usedJSHeapSize: Math.min(used, total)
      };
    });
  }

  globalThis.performance = performance;

  var location = Object.create(Location.prototype);

  function applyLocation(spec) {
    location.href = spec.href;
    location.protocol = spec.protocol;
    location.host = spec.host;
    location.hostname = spec.hostname;
    location.port = spec.port;
    location.pathname = spec.pathname;
    location.search = spec.search;
    location.hash = spec.hash;
    location.origin = spec.origin;
    location.ancestorOrigins = { length: 0, item: function () { return null; }, contains: function () { return false; } };
  }

  applyLocation(page.location);

  var history = Object.create(History.prototype);
  history.length = 1;
  history.state = null;
  history.scrollRestoration = "auto";

  var localStorage = storage();
  var sessionStorage = storage();

  var navigator = globalThis.navigator;

  function branded(interfaceName, members) {
    var Ctor = globalThis[interfaceName];

    if (typeof Ctor !== "function") {
      Ctor = tag(function () {
        throw new TypeError("Illegal constructor");
      }, interfaceName, globalThis.EventTarget);
    }

    var instance = Object.create(Ctor.prototype);

    Object.keys(members).forEach(function (key) {
      var value = members[key];
      if (typeof value === "function") {
        Ctor.prototype[key] = value;
        return;
      }
      instance[key] = value;
    });

    return instance;
  }

  function navigatorValue(name, entry) {
    var target = Object.getPrototypeOf(navigator) || navigator;

    if (typeof entry === "function") {
      Object.defineProperty(target, name, {
        value: entry,
        writable: true,
        enumerable: true,
        configurable: true
      });
      return;
    }

    Object.defineProperty(target, name, {
      get: nameAccessor(function () { return entry; }, "get", name),
      enumerable: true,
      configurable: true
    });
  }

  navigatorValue("javaEnabled", function javaEnabled() {
    spend(4);
    return false;
  });
  navigatorValue("taintEnabled", function taintEnabled() { return false; });
  navigatorValue("vibrate", function vibrate() { return true; });
  navigatorValue("getGamepads", function getGamepads() { return [null, null, null, null]; });
  navigatorValue("registerProtocolHandler", function registerProtocolHandler() {});
  navigatorValue("unregisterProtocolHandler", function unregisterProtocolHandler() {});
  navigatorValue("requestMediaKeySystemAccess", function requestMediaKeySystemAccess() {
    return Promise.reject(new Error("not supported"));
  });
  navigatorValue("sendBeacon", function sendBeacon(url, body) {
    requestCount += 1;
    startRequest({
      method: "POST",
      url: String(url),
      headers: {},
      body: body === undefined || body === null ? null : String(body),
      at: now(),
      source: "beacon"
    }, function () {});
    return true;
  });

  if (profile.battery) {
    var battery = branded("BatteryManager", {
      charging: profile.battery.charging,
      chargingTime: profile.battery.charging_time === null || profile.battery.charging_time === undefined
        ? Infinity
        : profile.battery.charging_time,
      dischargingTime: profile.battery.discharging_time === null || profile.battery.discharging_time === undefined
        ? Infinity
        : profile.battery.discharging_time,
      level: profile.battery.level,
      onchargingchange: null,
      onchargingtimechange: null,
      ondischargingtimechange: null,
      onlevelchange: null,
      addEventListener: function addEventListener() {},
      removeEventListener: function removeEventListener() {},
      dispatchEvent: function dispatchEvent() { return true; }
    });

    navigatorValue("getBattery", function getBattery() {
      return Promise.resolve(battery);
    });
  }

  var MimeType = typeof globalThis.MimeType === "function"
    ? globalThis.MimeType
    : tag(function MimeType() { throw new TypeError("Illegal constructor"); }, "MimeType");

  var MimeTypeArray = typeof globalThis.MimeTypeArray === "function"
    ? globalThis.MimeTypeArray
    : tag(function MimeTypeArray() { throw new TypeError("Illegal constructor"); }, "MimeTypeArray");

  MimeTypeArray.prototype.item = function (index) {
    var found = this[index >>> 0];
    return found === undefined ? null : found;
  };
  MimeTypeArray.prototype.namedItem = function (name) {
    var found = this[String(name)];
    return found === undefined ? null : found;
  };
  define(MimeTypeArray.prototype, "length", function () {
    return this.__count || 0;
  });

  var plugins = navigator.plugins;

  function pluginNamed(name) {
    for (var index = 0; plugins && index < plugins.length; index += 1) {
      if (plugins[index] && plugins[index].name === name) return plugins[index];
    }
    return null;
  }

  var mimeTypes = (profile.mime_types || []).map(function (entry) {
    var type = Object.create(MimeType.prototype);
    type.type = entry.type;
    type.suffixes = entry.suffixes;
    type.description = entry.description;
    type.enabledPlugin = pluginNamed(entry.plugin);
    return type;
  });

  var mimeTypeArray = Object.create(MimeTypeArray.prototype);
  mimeTypes.forEach(function (entry, index) {
    mimeTypeArray[index] = entry;
    Object.defineProperty(mimeTypeArray, entry.type, {
      value: entry,
      enumerable: false,
      configurable: true
    });
  });
  Object.defineProperty(mimeTypeArray, "__count", {
    value: mimeTypes.length,
    enumerable: false,
    configurable: true
  });

  navigatorValue("mimeTypes", mimeTypeArray);

  if (globalThis.PluginArray && !globalThis.PluginArray.prototype.refresh) {
    globalThis.PluginArray.prototype.refresh = function refresh() {};
  }

  navigatorValue("mediaSession", mediaSession);

  navigatorValue("geolocation", branded("Geolocation", {
    getCurrentPosition: function (onSuccess, onError) {
      if (typeof onError === "function") {
        settle(function () {
          onError({ code: 1, message: "User denied Geolocation", PERMISSION_DENIED: 1 });
        });
      }
    },
    watchPosition: function () { return 0; },
    clearWatch: function () {}
  }));

  if (profile.user_agent_data) {
    var brands = (profile.user_agent_data.brands || []).map(function (entry) {
      return { brand: entry.brand, version: entry.version };
    });

    var high = profile.user_agent_data.high_entropy || {};

    navigatorValue("userAgentData", branded("NavigatorUAData", {
      brands: brands,
      mobile: Boolean(profile.user_agent_data.mobile),
      platform: profile.user_agent_data.platform,
      toJSON: function () {
        return { brands: brands, mobile: this.mobile, platform: this.platform };
      },
      getHighEntropyValues: function (wanted) {
        var out = { brands: brands, mobile: this.mobile, platform: this.platform };
        (wanted || []).forEach(function (key) {
          if (out[key] !== undefined) return;
          if (high[key] !== undefined) out[key] = high[key];
          else hostMiss("userAgentData.getHighEntropyValues(" + key + ")");
        });
        return Promise.resolve(out);
      }
    }));
  }

  if (profile.connection) {
    navigatorValue("connection", branded("NetworkInformation", {
      downlink: profile.connection.downlink,
      effectiveType: profile.connection.effective_type,
      rtt: profile.connection.rtt,
      saveData: Boolean(profile.connection.save_data),
      onchange: null,
      addEventListener: function () {},
      removeEventListener: function () {}
    }));
  }

  var devices = (profile.media_devices || []).map(function (entry) {
    return {
      kind: entry.kind,
      label: entry.label,
      deviceId: entry.device_id,
      groupId: entry.group_id,
      toJSON: function () { return this; }
    };
  });

  navigatorValue("mediaDevices", branded("MediaDevices", {
    enumerateDevices: function () {
      if (!devices.length) hostMiss("mediaDevices.enumerateDevices");
      return Promise.resolve(devices.slice());
    },
    getSupportedConstraints: function () {
      return { width: true, height: true, aspectRatio: true, frameRate: true, facingMode: true };
    },
    getUserMedia: function () {
      return Promise.reject(new Error("Permission denied"));
    },
    addEventListener: function () {},
    removeEventListener: function () {}
  }));

  var storageEstimate = profile.storage || {};

  navigatorValue("storage", branded("StorageManager", {
    estimate: function () {
      if (profile.storage === undefined) hostMiss("storage.estimate");
      cost(1);

      var usage = storageEstimate.usage === undefined ? 0 : storageEstimate.usage;
      var answer = {
        quota: storageEstimate.quota === undefined ? 0 : storageEstimate.quota,
        usage: usage,
        usageDetails: usage > 0 ? { indexedDB: usage } : {}
      };

      return Promise.resolve(answer);
    },
    persisted: function () { return Promise.resolve(false); }
  }));

  navigatorValue("serviceWorker", branded("ServiceWorkerContainer", {
    controller: null,
    ready: new Promise(function () {}),
    register: function () { return Promise.reject(new Error("not supported")); },
    getRegistration: function () { return Promise.resolve(undefined); },
    getRegistrations: function () { return Promise.resolve([]); },
    addEventListener: function () {},
    removeEventListener: function () {}
  }));

  navigatorValue("credentials", branded("CredentialsContainer", {
    get: function () { return Promise.resolve(null); },
    store: function () { return Promise.resolve(); },
    preventSilentAccess: function () { return Promise.resolve(); }
  }));

  navigatorValue("clipboard", branded("Clipboard", {
    readText: function () { return Promise.reject(new Error("Read permission denied.")); },
    writeText: function () { return Promise.resolve(); }
  }));

  navigatorValue("bluetooth", branded("Bluetooth", {
    getAvailability: function () { return Promise.resolve(false); },
    requestDevice: function () { return Promise.reject(new Error("Bluetooth adapter not available.")); }
  }));

  navigatorValue("wakeLock", branded("WakeLock", { request: function () { return Promise.reject(new Error("not allowed")); } }));
  navigatorValue("locks", branded("LockManager", { query: function () { return Promise.resolve({ held: [], pending: [] }); } }));
  navigatorValue("webkitTemporaryStorage", branded("DeprecatedStorageQuota", { queryUsageAndQuota: function () {} }));
  navigatorValue("webkitPersistentStorage", branded("DeprecatedStorageQuota", { queryUsageAndQuota: function () {} }));
  navigatorValue("scheduling", { isInputPending: function () { return false; } });
  navigatorValue("userActivation", branded("UserActivation", { hasBeenActive: true, isActive: false }));

  if (profile.intl && profile.intl.time_zone) {
    var resolved = {
      locale: profile.intl.locale,
      calendar: profile.intl.calendar,
      numberingSystem: profile.intl.numbering_system,
      timeZone: profile.intl.time_zone,
      hourCycle: profile.intl.hour_cycle
    };

    var RealDateTimeFormat = Intl.DateTimeFormat;

    var patched = function DateTimeFormat(locales, options) {
      var instance = new RealDateTimeFormat(locales, options);
      var inner = instance.resolvedOptions;

      instance.resolvedOptions = function resolvedOptions() {
        var answer = inner.call(instance);
        answer.timeZone = resolved.timeZone;
        if (!locales) {
          answer.locale = resolved.locale;
          answer.calendar = resolved.calendar;
          answer.numberingSystem = resolved.numberingSystem;
          if (resolved.hourCycle) answer.hourCycle = resolved.hourCycle;
        }
        return answer;
      };

      return instance;
    };

    patched.supportedLocalesOf = RealDateTimeFormat.supportedLocalesOf;
    patched.prototype = RealDateTimeFormat.prototype;
    Intl.DateTimeFormat = patched;
    masked.push("Intl.DateTimeFormat");

    var offsetMinutes = profile.intl.timezone_offset || 0;
    var realOffset = RealDate.prototype.getTimezoneOffset;
    void realOffset;
    RealDate.prototype.getTimezoneOffset = function getTimezoneOffset() {
      spend(4);
      return offsetMinutes;
    };
    masked.push("Date.prototype.getTimezoneOffset");
  }

  var chromeShape = profile.chrome;

  if (chromeShape) {
    var chromeParts = {
      app: chromeShape.app || { isInstalled: false },
      csi: function csi() {
        return { onloadT: Math.floor(epoch) + 300, startE: Math.floor(epoch), pageT: elapsed(), tran: 15 };
      },
      loadTimes: function loadTimes() {
        return {
          requestTime: epoch / 1000,
          startLoadTime: epoch / 1000,
          commitLoadTime: epoch / 1000 + 0.1,
          finishDocumentLoadTime: epoch / 1000 + 0.3,
          finishLoadTime: epoch / 1000 + 0.4,
          firstPaintTime: epoch / 1000 + 0.35,
          navigationType: "Other",
          wasFetchedViaSpdy: true,
          wasNpnNegotiated: true,
          npnNegotiatedProtocol: "h2",
          wasAlternateProtocolAvailable: false,
          connectionInfo: "h2"
        };
      }
    };

    chromeParts.runtime = chromeShape.runtime || {};

    var chromeOrder = Array.isArray(chromeShape.keys) && chromeShape.keys.length
      ? chromeShape.keys
      : ["loadTimes", "csi", "app"];

    globalThis.chrome = {};
    chromeOrder.forEach(function (name) {
      if (chromeParts[name] !== undefined) globalThis.chrome[name] = chromeParts[name];
    });
  }

  var visualViewport = Object.create(VisualViewport.prototype);
  visualViewport.offsetLeft = 0;
  visualViewport.offsetTop = 0;
  visualViewport.pageLeft = 0;
  visualViewport.pageTop = 0;
  visualViewport.scale = 1;
  define(visualViewport, "width", function () { return globalThis.innerWidth; });
  define(visualViewport, "height", function () { return globalThis.innerHeight; });

  var crypto = Object.create(Crypto.prototype);

  var screenOrientation = profile.orientation
    ? { angle: profile.orientation.angle, type: profile.orientation.type,
        lock: function () { return Promise.reject(new Error("not supported")); },
        unlock: function () {}, addEventListener: function () {}, removeEventListener: function () {} }
    : null;

  if (screenOrientation && globalThis.screen) {
    Object.defineProperty(globalThis.screen, "orientation", {
      value: screenOrientation,
      enumerable: true,
      configurable: true
    });
  }

  globalThis.document = document;
  globalThis.location = location;
  globalThis.history = history;
  globalThis.localStorage = localStorage;
  globalThis.sessionStorage = sessionStorage;
  globalThis.XMLHttpRequest = XMLHttpRequest;
  globalThis.fetch = fetchImpl;
  globalThis.crypto = crypto;
  globalThis.speechSynthesis = speechSynthesis;
  globalThis.visualViewport = visualViewport;
  var databases = Object.create(null);

  function idbRequest() {
    var request = {
      result: null,
      error: null,
      readyState: "pending",
      onsuccess: null,
      onerror: null,
      onupgradeneeded: null,
      __handlers: {},
      addEventListener: function (kind, handler) {
        if (typeof handler !== "function") return;
        if (!this.__handlers[kind]) this.__handlers[kind] = [];
        this.__handlers[kind].push(handler);
      },
      removeEventListener: function () {}
    };

    request.__fire = function (kind) {
      request.readyState = "done";
      var event = { type: kind, target: request, currentTarget: request };
      var direct = request["on" + kind];
      if (typeof direct === "function") direct.call(request, event);
      (request.__handlers[kind] || []).forEach(function (handler) { handler.call(request, event); });
    };

    return request;
  }

  function idbStore(store) {
    return {
      name: store.name,
      keyPath: store.keyPath,
      indexNames: [],
      put: function (value, key) {
        var request = idbRequest();
        var id = key !== undefined ? String(key) : String(store.next++);
        store.rows[id] = value;
        request.result = id;
        settle(function () { request.__fire("success"); });
        return request;
      },
      add: function (value, key) {
        return this.put(value, key);
      },
      get: function (key) {
        var request = idbRequest();
        request.result = store.rows[String(key)];
        settle(function () { request.__fire("success"); });
        return request;
      },
      getAll: function () {
        var request = idbRequest();
        request.result = Object.keys(store.rows).map(function (key) { return store.rows[key]; });
        settle(function () { request.__fire("success"); });
        return request;
      },
      getAllKeys: function () {
        var request = idbRequest();
        request.result = Object.keys(store.rows);
        settle(function () { request.__fire("success"); });
        return request;
      },
      count: function () {
        var request = idbRequest();
        request.result = Object.keys(store.rows).length;
        settle(function () { request.__fire("success"); });
        return request;
      },
      delete: function (key) {
        var request = idbRequest();
        delete store.rows[String(key)];
        settle(function () { request.__fire("success"); });
        return request;
      },
      clear: function () {
        var request = idbRequest();
        store.rows = {};
        settle(function () { request.__fire("success"); });
        return request;
      },
      createIndex: function () { return { name: "", keyPath: "" }; },
      index: function () { return { get: function () { return idbRequest(); } }; }
    };
  }

  function settle(run) {
    Promise.resolve().then(run);
  }

  function idbDatabase(name, held) {
    return {
      name: name,
      version: held.version,
      objectStoreNames: Object.keys(held.stores),
      createObjectStore: function (store, options) {
        held.stores[store] = { name: store, keyPath: (options && options.keyPath) || null, rows: {}, next: 1 };
        this.objectStoreNames = Object.keys(held.stores);
        return idbStore(held.stores[store]);
      },
      deleteObjectStore: function (store) {
        delete held.stores[store];
        this.objectStoreNames = Object.keys(held.stores);
      },
      transaction: function (names) {
        var wanted = Array.isArray(names) ? names[0] : names;
        var transaction = {
          objectStore: function (store) {
            var target = held.stores[store] || held.stores[wanted];
            if (!target) {
              target = { name: store, keyPath: null, rows: {}, next: 1 };
              held.stores[store] = target;
            }
            return idbStore(target);
          },
          abort: function () {},
          commit: function () {},
          oncomplete: null,
          onerror: null,
          addEventListener: function (kind, handler) {
            if (kind === "complete" && typeof handler === "function") settle(function () { handler({ type: "complete" }); });
          },
          removeEventListener: function () {}
        };

        settle(function () {
          if (typeof transaction.oncomplete === "function") transaction.oncomplete({ type: "complete" });
        });

        return transaction;
      },
      close: function () {},
      addEventListener: function () {},
      removeEventListener: function () {}
    };
  }

  globalThis.indexedDB = branded("IDBFactory", {
    open: function (name, version) {
      spend(2);
      var key = String(name);
      var request = idbRequest();
      var fresh = !databases[key];

      if (fresh) databases[key] = { version: version || 1, stores: {} };
      var held = databases[key];
      if (version && version > held.version) held.version = version;

      var database = idbDatabase(key, held);
      request.result = database;

      settle(function () {
        if (fresh) {
          request.__fire("upgradeneeded");
          database.objectStoreNames = Object.keys(held.stores);
        }
        request.__fire("success");
      });

      return request;
    },
    databases: function () {
      return Promise.resolve(
        Object.keys(databases).map(function (name) { return { name: name, version: databases[name].version }; })
      );
    },
    deleteDatabase: function (name) {
      var request = idbRequest();
      delete databases[String(name)];
      settle(function () { request.__fire("success"); });
      return request;
    },
    cmp: function (first, second) {
      return first < second ? -1 : first > second ? 1 : 0;
    }
  });
  globalThis.CSS = {
    supports: function () { return true; },
    escape: function (text) { return String(text); }
  };
  globalThis.getComputedStyle = function getComputedStyle(node) {
    spend(2);
    var style = Object.create(CSSStyleDeclaration.prototype);
    style.fontSize = "16px";
    style.fontFamily = "-apple-system, sans-serif";
    style.display = node && node.hidden ? "none" : "block";
    style.visibility = "visible";
    style.width = ((node && node.offsetWidth) || 0) + "px";
    style.height = ((node && node.offsetHeight) || 0) + "px";
    return style;
  };
  globalThis[Symbol.for("wre.scroll")] = { x: 0, y: 0 };

  function moveScroll(x, y) {
    var limit = Math.max(0, (document.documentElement.scrollHeight || 0) - (globalThis.innerHeight || 0));
    var top = Math.max(0, Math.min(Number(y) || 0, limit || Number(y) || 0));
    var left = Math.max(0, Number(x) || 0);

    if (top === globalThis[Symbol.for("wre.scroll")].y && left === globalThis[Symbol.for("wre.scroll")].x) return;

    globalThis[Symbol.for("wre.scroll")].x = left;
    globalThis[Symbol.for("wre.scroll")].y = top;

    try {
      globalThis.scrollX = left;
      globalThis.scrollY = top;
      globalThis.pageXOffset = left;
      globalThis.pageYOffset = top;
    } catch (error) {
      void error;
    }
    if (document.documentElement) document.documentElement.scrollTop = top;
    if (document.body) document.body.scrollTop = top;

    var made = new Event("scroll", { bubbles: false });
    made.isTrusted = true;
    made.target = document;
    document.dispatchEvent(made);
    globalThis.dispatchEvent(made);
  }

  globalThis.scrollTo = function scrollTo(x, y) {
    if (x && typeof x === "object") return moveScroll(x.left, x.top);
    return moveScroll(x, y);
  };

  globalThis.scrollBy = function scrollBy(x, y) {
    if (x && typeof x === "object") {
      return moveScroll((globalThis.scrollX || 0) + (x.left || 0), (globalThis.scrollY || 0) + (x.top || 0));
    }
    return moveScroll((globalThis.scrollX || 0) + (Number(x) || 0), (globalThis.scrollY || 0) + (Number(y) || 0));
  };
  globalThis.alert = function alert() {};
  globalThis.confirm = function confirm() { return false; };
  globalThis.prompt = function prompt() { return null; };
  globalThis.open = function open() { return null; };
  globalThis.close = function close() {};
  globalThis.focus = function focus() {};
  globalThis.blur = function blur() {};
  globalThis.stop = function stop() {};
  globalThis.print = function print() {};
  globalThis.postMessage = function postMessage() {};
  globalThis.structuredClone = function structuredClone(entry) {
    return JSON.parse(JSON.stringify(entry));
  };
  globalThis.reportError = function reportError() {};
  globalThis.matchMedia = globalThis.matchMedia || function matchMedia() {
    return { matches: false, media: "", addListener: function () {}, removeListener: function () {} };
  };

  globalThis.window = globalThis;
  globalThis.self = globalThis;
  globalThis.top = globalThis;
  globalThis.parent = globalThis;
  globalThis.frames = globalThis;
  globalThis.length = 0;
  globalThis.closed = false;
  globalThis.opener = null;
  globalThis.name = "";
  globalThis.origin = page.location.origin;
  globalThis.isSecureContext = page.location.protocol === "https:";
  globalThis.crossOriginIsolated = false;

  if (!globalThis.crossOriginIsolated) {
    try {
      delete globalThis.SharedArrayBuffer;
    } catch (error) {
      void error;
    }
  }

  globalThis.originAgentCluster = false;
  globalThis.screenLeft = globalThis.screenX;
  globalThis.screenTop = globalThis.screenY;
  globalThis.pageXOffset = 0;
  globalThis.pageYOffset = 0;
  globalThis.scrollX = 0;
  globalThis.scrollY = 0;
  globalThis.onerror = null;
  globalThis.onload = null;

  function attribute(node, name, entry) {
    node.setAttribute(name, entry);
  }

  var pendingInputs = [];
  var finishParse = function () {};

  var FIELD_TAGS = { input: 1, textarea: 1, select: 1, button: 1 };

  function parseDocument(markup, html, head, body) {
    var held = friction;
    friction = 0;

    var container = element("html");
    parseFragment(String(markup), container);

    friction = held;

    var root = container.childNodes.filter(function (node) {
      return node.nodeType === 1 && node.localName === "html";
    })[0] || container;

    var adopt = function (target, source) {
      source.childNodes.slice().forEach(function (child) {
        target.appendChild(child);
      });
    };

    var parsedHead = root.childNodes.filter(function (node) {
      return node.nodeType === 1 && node.localName === "head";
    })[0];

    var parsedBody = root.childNodes.filter(function (node) {
      return node.nodeType === 1 && node.localName === "body";
    })[0];

    if (parsedHead) adopt(head, parsedHead);
    if (parsedBody) adopt(body, parsedBody);

    if (!parsedHead && !parsedBody) adopt(body, root);
    else {
      root.childNodes.slice().forEach(function (child) {
        if (child === parsedHead || child === parsedBody) return;
        if (child.nodeType === 1) body.appendChild(child);
      });
    }

    Object.keys(root.__attributes || {}).forEach(function (name) {
      html.setAttribute(name, root.__attributes[name]);
    });

    if (parsedBody) {
      Object.keys(parsedBody.__attributes || {}).forEach(function (name) {
        body.setAttribute(name, parsedBody.__attributes[name]);
      });
    }

    var all = descendants(document);
    var scripts = [];
    var forms = [];
    var images = [];
    var links = [];
    var fields = [];
    var sheets = [];

    all.forEach(function (node) {
      var name = node.localName;

      if (name === "script") {
        node.async = node.hasAttribute("async");
        node.defer = node.hasAttribute("defer");
        hide(node, "__activated", true);
        scripts.push(node);
        return;
      }

      if (name === "link") {
        hide(node, "__activated", true);
        if (/stylesheet/i.test(node.getAttribute("rel") || "")) sheets.push(styleSheetOf(node));
        return;
      }

      if (name === "style") {
        sheets.push(styleSheetOf(node));
        return;
      }

      if (name === "form") {
        node.elements = [];
        forms.push(node);
        return;
      }

      if (name === "img") {
        images.push(node);
        return;
      }

      if (name === "a" && node.hasAttribute("href")) {
        links.push(node);
        return;
      }

      if (!FIELD_TAGS[name]) return;

      var hidden = node.getAttribute("type") === "hidden" || node.hasAttribute("hidden");

      node.form = node.closest("form");
      if (node.form) node.form.elements.push(node);

      node.labels = [];
      node.offsetParent = hidden ? null : body;
      node.offsetWidth = hidden ? 0 : 180;
      node.offsetHeight = hidden ? 0 : 22;
      node.required = node.hasAttribute("required");

      fields.push(node);
    });

    return {
      scripts: scripts,
      forms: forms,
      images: images,
      links: links,
      fields: fields,
      sheets: sheets
    };
  }

  function build(spec) {
    document.URL = spec.location.href;
    document.documentURI = spec.location.href;
    documentBase = spec.location.href;
    document.baseURI = spec.location.href;
    document.referrer = spec.referrer || "";
    document.title = spec.title || "";
    document.domain = spec.location.hostname;
    document.readyState = "loading";
    document.lastModified = new RealDate(epoch).toLocaleString("en-US");

    var documentProperties = profile.document || {};
    Object.keys(documentProperties).forEach(function (key) {
      document[key] = documentProperties[key];
    });

    setup(document, document, "#document", 9);
    document.ownerDocument = null;

    var html = element("html");
    var head = element("head");
    var body = element("body");

    document.appendChild(html);
    html.appendChild(head);
    html.appendChild(body);

    document.documentElement = html;
    document.head = head;
    document.body = body;
    document.scrollingElement = html;
    document.activeElement = body;
    document.defaultView = globalThis;
    document.location = location;
    document.currentScript = null;

    document.implementation = {
      hasFeature: function () { return true; },
      createDocumentType: function (name, publicId, systemId) {
        return { name: name, publicId: publicId || "", systemId: systemId || "", nodeType: 10 };
      },
      createHTMLDocument: function (title) {
        var made = Object.create(Document.prototype);
        setup(made, made, "#document", 9);
        made.ownerDocument = null;

        var root = element("html");
        var top = element("head");
        var content = element("body");

        made.appendChild(root);
        root.appendChild(top);
        root.appendChild(content);

        made.documentElement = root;
        made.head = top;
        made.body = content;
        made.location = location;
        made.defaultView = null;
        made.implementation = document.implementation;

        if (title) {
          var heading = element("title");
          heading.appendChild(document.createTextNode(String(title)));
          top.appendChild(heading);
        }

        return made;
      },
    };

    var measured = (spec.geometry && spec.geometry.client) || null;

    html.clientWidth = measured && measured.width ? measured.width : globalThis.innerWidth;
    html.clientHeight = measured && measured.height ? measured.height : globalThis.innerHeight;
    html.offsetWidth = html.clientWidth;
    html.offsetHeight = html.clientHeight;
    html.scrollWidth = measured && measured.scrollWidth ? measured.scrollWidth : html.clientWidth;
    html.scrollHeight = measured && measured.scrollHeight ? measured.scrollHeight : html.clientHeight;
    body.clientWidth = globalThis.innerWidth;
    body.clientHeight = globalThis.innerHeight;
    body.offsetWidth = globalThis.innerWidth;
    body.offsetHeight = globalThis.innerHeight;
    body.offsetParent = null;
    hide(html, "__innerHTML", spec.html || "");

    var parsed = spec.html ? parseDocument(spec.html, html, head, body) : null;

    var scripts = parsed
      ? parsed.scripts
      : (spec.scripts || []).map(function (src) {
          var node = element("script");
          if (src && src !== "[inline]") attribute(node, "src", src);
          node.async = false;
          node.defer = false;
          head.appendChild(node);
          return node;
        });

    document.scripts = collection(scripts);

    var running = scripts.filter(function (node) { return node.src === spec.current_script; })[0];
    document.currentScript = running || (scripts.length ? scripts[scripts.length - 1] : null);

    var forms = parsed
      ? parsed.forms
      : (spec.forms || []).map(function (entry) {
          var node = element("form");
          Object.keys(entry.attributes || {}).forEach(function (name) {
            attribute(node, name, entry.attributes[name]);
          });
          body.appendChild(node);
          return node;
        });

    var allInputs = spec.inputs || [];
    var parsedInputs = typeof spec.inputs_parsed === "number" ? spec.inputs_parsed : allInputs.length;

    function addInput(entry) {
      var node = element(entry.tag || "input");
      Object.keys(entry.attributes || {}).forEach(function (name) {
        attribute(node, name, entry.attributes[name]);
      });

      node.labels = [];
      for (var index = 0; index < (entry.labels || 0); index += 1) {
        node.labels.push(element("label"));
      }

      node.form = entry.form >= 0 && forms[entry.form] ? forms[entry.form] : (forms[0] || null);
      node.offsetParent = entry.visible === false ? null : body;
      node.offsetWidth = entry.visible === false ? 0 : 180;
      node.offsetHeight = entry.visible === false ? 0 : 22;
      node.required = Boolean(entry.attributes && entry.attributes.required !== undefined);
      node.type = (entry.attributes && entry.attributes.type) || (entry.tag === "input" ? "text" : entry.tag);

      if (node.form) {
        node.form.elements.push(node);
        node.form.appendChild(node);
      } else {
        body.appendChild(node);
      }
    }

    if (parsed) {
      var held = parsed.fields.slice(parsedInputs).map(function (node) {
        return { node: node, parent: node.parentNode, at: node.parentNode ? node.parentNode.childNodes.indexOf(node) : -1 };
      });

      held.forEach(function (entry) {
        if (entry.parent) entry.parent.removeChild(entry.node);
      });

      finishParse = function () {
        held.forEach(function (entry) {
          if (!entry.parent) return;
          if (entry.at >= 0 && entry.at <= entry.parent.childNodes.length) {
            entry.node.parentNode = entry.parent;
            entry.parent.childNodes.splice(entry.at, 0, entry.node);
          } else {
            entry.parent.appendChild(entry.node);
          }
        });
        held = [];
        document.all = collection(descendants(document), HTMLAllCollection);
        layoutDocument();
      };
    } else {
      allInputs.slice(0, parsedInputs).forEach(addInput);
      pendingInputs = allInputs.slice(parsedInputs);
      finishParse = function () {
        pendingInputs.forEach(addInput);
        pendingInputs = [];
        document.all = collection(descendants(document), HTMLAllCollection);
        layoutDocument();
      };
    }

    document.forms = collection(forms);
    document.images = collection(parsed ? parsed.images : []);
    document.links = collection(parsed ? parsed.links : []);
    document.all = collection(descendants(document), HTMLAllCollection);
    layoutDocument();
    activationEnabled = true;
    document.embeds = collection([]);
    document.plugins = collection([]);
    document.styleSheets = collection(parsed ? parsed.sheets : [], StyleSheetList);
    document.fonts = {
      ready: Promise.resolve(),
      check: function () { return true; },
      load: function () { return Promise.resolve([]); },
      values: function () { return []; },
      size: 0
    };
  }

  build(page);

  var charges = [];

  function chargeOn(name, property, cost) {
    var stored;
    var applied = false;

    function charge() {
      if (applied) return;
      applied = true;
      skew += cost;
    }

    charges.push(name + "." + property);

    var current = globalThis[name];

    Object.defineProperty(globalThis, name, {
      configurable: true,
      enumerable: true,
      get: function () { return stored; },
      set: function (entry) {
        stored = entry;
        if (!entry || typeof entry !== "object") return;

        var held = entry[property];

        try {
          Object.defineProperty(entry, property, {
            configurable: true,
            enumerable: true,
            get: function () { return held; },
            set: function (fresh) {
              held = fresh;
              charge();
            }
          });
          if (held !== undefined) charge();
        } catch (error) {
          void error;
        }
      }
    });

    if (current !== undefined) globalThis[name] = current;
  }

  function evaluate(source, name) {
    try {
      (0, eval)(source);
    } catch (error) {
      try {
        new Function(source)();
      } catch (nested) {
        void name;
      }
    }
  }

  var SCRIPT_TYPES = {
    "": 1, "module": 1,
    "text/javascript": 1, "text/ecmascript": 1, "text/jscript": 1, "text/livescript": 1,
    "text/x-ecmascript": 1, "text/x-javascript": 1,
    "application/javascript": 1, "application/ecmascript": 1, "application/x-ecmascript": 1,
    "application/x-javascript": 1,
    "text/javascript1.0": 1, "text/javascript1.1": 1, "text/javascript1.2": 1,
    "text/javascript1.3": 1, "text/javascript1.4": 1, "text/javascript1.5": 1
  };

  function runsAsScript(node) {
    var type = String(node.getAttribute("type") || "").trim().toLowerCase();
    if (type) return Boolean(SCRIPT_TYPES[type.split(";")[0].trim()]);

    var language = String(node.getAttribute("language") || "").trim().toLowerCase();
    if (language) return Boolean(SCRIPT_TYPES["text/" + language]);

    return true;
  }

  runInserted = function (node) {
    var name = node.localName;
    var reference = node.getAttribute(name === "link" ? "href" : "src") || "";

    if (name === "script") {
      if (!runsAsScript(node)) return;

      var inline = node.text || node.textContent || "";

      if (!reference) {
        if (inline) evaluate(inline, "inserted");
        return;
      }

      requestCount += 1;
      startRequest({
        method: "GET",
        url: resolveUrl(reference, location.href),
        headers: {},
        body: null,
        at: now(),
        source: "script",
      }, function (answer) {
        if (answer && answer.body) evaluate(String(answer.body), reference);
      });
      return;
    }

    if (!reference) return;
    if (name === "link" && !/stylesheet|icon|preload/i.test(node.getAttribute("rel") || "")) return;

    requestCount += 1;
    startRequest({
      method: "GET",
      url: resolveUrl(reference, location.href),
      headers: {},
      body: null,
      at: now(),
      source: name,
    }, function () {});
  };

  var FOCUSABLE = { input: 1, textarea: 1, select: 1, button: 1, a: 1 };

  function laidOut(node) {
    if (LAYOUT_SKIP[node.localName]) return false;
    if (node.hidden) return false;
    if (node.localName === "input" && String(node.getAttribute("type") || "").toLowerCase() === "hidden") return false;

    var style = String(node.getAttribute("style") || "");
    if (/display\s*:\s*none/i.test(style)) return false;

    return true;
  }

  function ownText(node) {
    var text = "";

    for (var index = 0; index < node.childNodes.length; index += 1) {
      var child = node.childNodes[index];
      if (child.nodeType === 3) text += String(child.nodeValue || "");
    }

    return text.replace(/\s+/g, " ").trim();
  }

  function measureBlock(node, width) {
    if (!laidOut(node)) return 0;

    if (LAYOUT_FIELD[node.localName]) return node.localName === "textarea" ? 60 : 24;
    if (node.localName === "img" || node.localName === "svg" || node.localName === "canvas") {
      return Number(node.getAttribute("height")) || 32;
    }
    if (node.localName === "hr") return 21;
    if (node.localName === "br") return LINE_HEIGHT;

    var height = 0;
    var line = 0;

    for (var index = 0; index < node.childNodes.length; index += 1) {
      var child = node.childNodes[index];

      if (child.nodeType === 3) {
        var text = String(child.nodeValue || "").replace(/\s+/g, " ").trim();
        if (text) line += text.length * CHAR_WIDTH;
        continue;
      }

      if (child.nodeType !== 1 || !laidOut(child)) continue;

      if (LAYOUT_INLINE[child.localName] || LAYOUT_FIELD[child.localName]) {
        line += LAYOUT_FIELD[child.localName] ? 184 : ownText(child).length * CHAR_WIDTH + 8;
        continue;
      }

      if (line > 0) {
        height += Math.ceil(line / Math.max(1, width)) * LINE_HEIGHT;
        line = 0;
      }

      height += measureBlock(child, Math.max(40, width - 16));
    }

    if (line > 0) height += Math.ceil(line / Math.max(1, width)) * LINE_HEIGHT;
    if (height === 0) height = ownText(node) ? LINE_HEIGHT : 0;

    return height;
  }

  function placeBlock(node, left, top, width) {
    var y = top;
    var lineLeft = left;
    var lineTop = top;
    var lineTall = 0;

    var endLine = function () {
      if (lineTall === 0) return;
      y += lineTall;
      lineLeft = left;
      lineTop = y;
      lineTall = 0;
    };

    for (var index = 0; index < node.childNodes.length; index += 1) {
      var child = node.childNodes[index];

      if (child.nodeType === 3) {
        var text = String(child.nodeValue || "").replace(/\s+/g, " ").trim();
        if (!text) continue;
        lineLeft += text.length * CHAR_WIDTH;
        lineTall = Math.max(lineTall, LINE_HEIGHT);
        if (lineLeft > left + width) {
          lineLeft = left;
          y += LINE_HEIGHT;
          lineTop = y;
        }
        continue;
      }

      if (child.nodeType !== 1) continue;

      if (!laidOut(child)) {
        hide(child, "__box", { left: 0, top: 0, width: 0, height: 0 });
        placeBlock(child, 0, 0, 0);
        continue;
      }

      if (LAYOUT_INLINE[child.localName] || LAYOUT_FIELD[child.localName]) {
        var inlineWidth = LAYOUT_FIELD[child.localName]
          ? (child.localName === "textarea" ? 260 : 180)
          : Math.min(width, ownText(child).length * CHAR_WIDTH + 8);
        var inlineHeight = LAYOUT_FIELD[child.localName]
          ? (child.localName === "textarea" ? 60 : 24)
          : LINE_HEIGHT;

        if (lineLeft + inlineWidth > left + width && lineLeft > left) {
          lineLeft = left;
          y += lineTall || LINE_HEIGHT;
          lineTop = y;
          lineTall = 0;
        }

        hide(child, "__box", { left: lineLeft, top: lineTop, width: inlineWidth, height: inlineHeight });
        placeBlock(child, lineLeft, lineTop, inlineWidth);

        lineLeft += inlineWidth + 6;
        lineTall = Math.max(lineTall, inlineHeight);
        continue;
      }

      endLine();

      var blockWidth = Math.max(40, width - 16);
      var blockHeight = measureBlock(child, blockWidth);

      hide(child, "__box", { left: left + 8, top: y, width: blockWidth, height: blockHeight });
      placeBlock(child, left + 8, y, blockWidth);

      y += blockHeight + 8;
    }

    endLine();
    return y - top;
  }

  function layoutDocument() {
    var captured = (page.geometry && page.geometry.boxes) || null;

    if (captured) {
      layoutCaptured(captured);
      return;
    }

    var viewport = Math.max(320, globalThis.innerWidth || 1200);
    var column = Math.min(1180, viewport - 48);
    var left = Math.round((viewport - column) / 2);

    if (document.body) {
      var tall = measureBlock(document.body, column);
      hide(document.body, "__box", { left: left, top: 0, width: column, height: tall });
      placeBlock(document.body, left, 24, column);
    }
  }

  function layoutCaptured(captured) {
    var y = 88;
    var counts = Object.create(null);

    var keyFor = function (node) {
      var id = node.getAttribute && node.getAttribute("id");
      if (id) return "#" + id;

      var name = node.getAttribute && node.getAttribute("name");
      if (name) return node.localName + "[name=" + name + "]";

      var tag = node.localName;
      counts[tag] = (counts[tag] || 0) + 1;
      return tag + ":" + (counts[tag] - 1);
    };

    var place = function (node, depth) {
      for (var index = 0; index < node.childNodes.length; index += 1) {
        var child = node.childNodes[index];
        if (child.nodeType !== 1) continue;

        var known = captured[keyFor(child)];

        if (known && child !== document.documentElement && child !== document.body) {
          hide(child, "__box", {
            left: known.left,
            top: known.top,
            width: known.width,
            height: known.height
          });
          place(child, depth + 1);
          continue;
        }

        var width = child.offsetWidth || 0;
        var height = child.offsetHeight || 0;

        if (width > 0 && height > 0 && child !== document.documentElement && child !== document.body) {
          hide(child, "__box", { left: 16 + depth * 4, top: y, width: width, height: height });
          y += height + 10;
        }

        place(child, depth + 1);
      }
    };

    place(document.documentElement, 0);
  }

  function focusNode(node) {
    var previous = document.activeElement;
    if (!node || previous === node) return;

    var Ctor = globalThis.FocusEvent || Event;

    var deliver = function (kind, at) {
      var made = new Ctor(kind, {});
      made.isTrusted = true;
      made.target = at;
      made.srcElement = at;
      at.dispatchEvent(made);
      document.dispatchEvent(made);
    };

    if (previous && previous !== document.body) deliver("blur", previous);
    document.activeElement = node;
    deliver("focus", node);
  }

  function applyTyping(event, target) {
    if (!target || target === globalThis) return;
    if (target.localName !== "input" && target.localName !== "textarea") return;

    var key = String(event.key || "");
    if (key.length !== 1) return;

    target.value = String(target.value === undefined ? "" : target.value) + key;

    var Ctor = globalThis.InputEvent || Event;
    var made = new Ctor("input", { data: key, inputType: "insertText", bubbles: true });
    made.isTrusted = true;
    made.target = target;
    made.srcElement = target;
    target.dispatchEvent(made);
    document.dispatchEvent(made);
  }

  function targetFor(type, options) {
    if (options.target === "window") return globalThis;
    if (options.target && options.target.nodeType) return options.target;
    if (/^key/.test(type)) return document.activeElement || document.body;
    if (typeof options.clientX === "number") {
      return document.elementFromPoint(options.clientX, options.clientY) || document.body;
    }
    return document.body;
  }

  function fire(type, detail) {
    var options = detail || {};
    var made;

    if (/^(mouse|click|dblclick|contextmenu)/.test(type)) made = new MouseEvent(type, options);
    else if (/^pointer/.test(type)) made = new PointerEvent(type, options);
    else if (/^touch/.test(type)) made = new TouchEvent(type, buildTouches(options));
    else if (/^key/.test(type)) made = new KeyboardEvent(type, keyDetail(options));
    else if (type === "wheel") made = new WheelEvent(type, options);
    else if (type === "deviceorientation") made = new DeviceOrientationEvent(type, options);
    else if (type === "devicemotion") made = new DeviceMotionEvent(type, options);
    else if (/^(focus|blur)/.test(type)) made = new FocusEvent(type, options);
    else made = new Event(type, options);

    made.isTrusted = true;
    made.timeStamp = elapsed();

    var target = targetFor(type, options);
    if (type === "mousedown" && target !== globalThis && FOCUSABLE[target.localName]) focusNode(target);

    made.target = target;
    made.srcElement = target;

    if (target !== globalThis) {
      target.dispatchEvent(made);
      if (target !== document.body) document.body.dispatchEvent(made);
      document.dispatchEvent(made);
      document.documentElement.dispatchEvent(made);
    }

    globalThis.dispatchEvent(made);

    if (type === "keypress") applyTyping(made, target);
    return true;
  }

  function keyDetail(options) {
    var key = String(options.key || "");
    var code = options.keyCode;

    if (options.charCode !== undefined) {
      var carried = { keyCode: options.charCode, which: options.charCode, charCode: options.charCode };
      Object.keys(options).forEach(function (name) {
        if (carried[name] === undefined) carried[name] = options[name];
      });
      return carried;
    }

    if (code === undefined) {
      if (key === " ") code = 32;
      else if (key === "Enter") code = 13;
      else if (key === "Tab") code = 9;
      else if (key === "Backspace") code = 8;
      else if (key.length === 1) code = key.toUpperCase().charCodeAt(0);
      else code = 0;
    }

    var filled = { keyCode: code, which: options.which === undefined ? code : options.which };

    Object.keys(options).forEach(function (name) {
      if (filled[name] === undefined) filled[name] = options[name];
    });

    if (filled.code === undefined && key.length === 1) {
      filled.code = /[a-z]/i.test(key) ? "Key" + key.toUpperCase() : "Digit" + key;
    }

    return filled;
  }

  function buildTouches(options) {
    var points = (options.touches || [options]).map(function (point) {
      return new Touch({
        identifier: point.identifier || 0,
        target: document.body,
        clientX: point.clientX || 0,
        clientY: point.clientY || 0,
        pageX: point.pageX || point.clientX || 0,
        pageY: point.pageY || point.clientY || 0,
        screenX: point.screenX || point.clientX || 0,
        screenY: point.screenY || point.clientY || 0,
        force: point.force === undefined ? 1 : point.force
      });
    });

    return { touches: points, targetTouches: points, changedTouches: points };
  }

  masked.push("Document.prototype");
  masked.push("Element.prototype");
  masked.push("Node.prototype");
  masked.push("globalThis");
  masked.push("navigator");
  masked.push("Navigator.prototype");
  masked.push("Screen.prototype");
  masked.push("Window.prototype");
  masked.push("performance");
  masked.push("Performance.prototype");
  masked.push("Storage.prototype");
  masked.push("StorageManager.prototype");
  masked.push("Location.prototype");
  masked.push("History.prototype");
  masked.push("Crypto.prototype");
  masked.push("SubtleCrypto.prototype");
  masked.push("CanvasRenderingContext2D.prototype");
  masked.push("WebGL2RenderingContext.prototype");
  masked.push("PluginArray.prototype");
  masked.push("Plugin.prototype");
  masked.push("MimeTypeArray.prototype");
  masked.push("MimeType.prototype");
  masked.push("Permissions.prototype");
  masked.push("PermissionStatus.prototype");
  masked.push("MediaQueryList.prototype");
  masked.push("HTMLMediaElement.prototype");
  masked.push("console");
  masked.push("chrome");
  masked.push("chrome.csi");
  masked.push("chrome.loadTimes");
  masked.push("navigator.plugins");
  masked.push("navigator.mimeTypes");
  masked.push("document.implementation");
  masked.push("document.fonts");
  masked.push("screen.orientation");

  delete globalThis.__wreProfileBlob;
  delete globalThis.__wrePageBlob;
  delete globalThis.__wreRequest;
  delete globalThis.__wreRequestStart;
  delete globalThis.__wreRequestTake;
  delete globalThis.__wreCookieRead;
  delete globalThis.__wreCookieWrite;
  delete globalThis.__wreCanvasImage;
  delete globalThis.__wreMeasureText;
  delete globalThis.__wreMiss;
  delete globalThis.__wreRealNow;
  delete globalThis.__wreEntropy;
  delete globalThis.__wreDigest;
  delete globalThis.__wreHeap;

  return {
    advance: function (ms) {
      var until = elapsed() + Math.max(0, Number(ms) || 0);
      var ran = runDue(until);
      reach(until);
      return ran;
    },
    stepTo: function (until) {
      return runOne(Number(until));
    },
    horizon: function (ms) {
      return elapsed() + Math.max(0, Number(ms) || 0);
    },
    reach: function (until) {
      return reach(until);
    },
    charge: function (ms) {
      skew += Math.max(0, Number(ms) || 0);
      return elapsed();
    },
    settle: function (rounds) {
      var ran = 0;
      for (var round = 0; round < (rounds || 8); round += 1) {
        var did = runDue(elapsed());
        ran += did;
        if (!did) break;
      }
      return ran;
    },
    fire: fire,
    parsed: function () {
      finishParse();
      return document.readyState;
    },
    ready: function (state) {
      if (state !== "loading") {
        finishParse();
        document.currentScript = null;
      }
      document.readyState = String(state);
      fire("readystatechange", { target: "document" });
      if (state === "interactive") fire("DOMContentLoaded", {});
      if (state === "complete") fire("load", { target: "window" });
      return document.readyState;
    },
    now: now,
    elapsed: elapsed,
    pending: function () {
      return timers.filter(function (timer) { return !timer.cancelled; }).length;
    },
    requests: function () {
      return requestCount;
    },
    chargeOn: chargeOn,
    charges: function () { return charges.slice(); },
    runningScript: function (src) {
      var wanted = String(src || "");
      var found = null;

      if (wanted) {
        var list = document.getElementsByTagName("script");
        for (var index = 0; index < list.length; index += 1) {
          var candidate = list[index].src || (list[index].getAttribute ? list[index].getAttribute("src") : "");
          if (candidate && String(candidate) === wanted) {
            found = list[index];
            break;
          }
        }
      }

      document.currentScript = found;
      return Boolean(found);
    },
    rate: function (factor) {
      return setRate(factor);
    },
    friction: function (ms) {
      friction = Math.max(0, Number(ms) || 0);
      return friction;
    },
    jump: function (ms) {
      skew += Math.max(0, Number(ms) || 0);
      return elapsed();
    },
    masked: function () {
      return masked.slice();
    },
    cookies: function () {
      return hostCookieRead();
    }
  };
})()
