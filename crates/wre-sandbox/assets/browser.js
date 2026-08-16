(function () {
  var profile = __wreProfileBlob();
  var page = __wrePageBlob();
  var hostRequest = __wreRequest;
  var hostCookieRead = __wreCookieRead;
  var hostCookieWrite = __wreCookieWrite;
  var hostCanvas = __wreCanvasImage;
  var hostMeasure = __wreMeasureText;
  var hostMiss = __wreMiss;
  var hostReal = __wreRealNow;

  var epoch = page.epoch;
  var realStart = hostReal();
  var offset = 0;
  var friction = page.friction || 0;
  var timers = [];
  var nextTimer = 1;
  var masked = [];

  function now() {
    return epoch + offset + (hostReal() - realStart);
  }

  function elapsed() {
    return offset + (hostReal() - realStart);
  }

  function spend(weight) {
    if (friction > 0) offset += friction * (weight || 1);
  }

  function define(holder, name, getter, setter) {
    Object.defineProperty(holder, name, {
      get: getter,
      set: setter,
      enumerable: true,
      configurable: true
    });
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
  };

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
    return this[index] || null;
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
    this.acceleration = source.acceleration || null;
    this.accelerationIncludingGravity = source.accelerationIncludingGravity || null;
    this.rotationRate = source.rotationRate || null;
    this.interval = source.interval || 16;
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
    return tokens(this.__node)[index] || null;
  };

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

  var Node = tag(function Node() {
    throw new TypeError("Illegal constructor");
  }, "Node", EventTargetBase);

  Node.ELEMENT_NODE = 1;
  Node.TEXT_NODE = 3;
  Node.DOCUMENT_NODE = 9;
  Node.DOCUMENT_FRAGMENT_NODE = 11;
  Node.prototype.ELEMENT_NODE = 1;
  Node.prototype.TEXT_NODE = 3;
  Node.prototype.DOCUMENT_NODE = 9;

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
    node.__attributes = {};
    node.className = "";
    node.id = "";
    node.textContent = "";
    node.__style = Object.create(CSSStyleDeclaration.prototype);
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

  Node.prototype.appendChild = function (child) {
    spend(1);
    if (child.parentNode) child.parentNode.removeChild(child);
    child.parentNode = this;
    this.childNodes.push(child);
    return child;
  };

  Node.prototype.insertBefore = function (child, reference) {
    spend(1);
    var at = reference ? this.childNodes.indexOf(reference) : -1;
    if (child.parentNode) child.parentNode.removeChild(child);
    child.parentNode = this;
    if (at === -1) this.childNodes.push(child);
    else this.childNodes.splice(at, 0, child);
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

  Node.prototype.cloneNode = function () {
    var copy = this.ownerDocument.createElement(this.localName);
    for (var name in this.__attributes) {
      if (Object.prototype.hasOwnProperty.call(this.__attributes, name)) {
        copy.setAttribute(name, this.__attributes[name]);
      }
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
      this[key] = text;
    }
  };

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
    return new DOMRect(0, 0, this.offsetWidth, this.offsetHeight);
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

  define(Element.prototype, "children", function () {
    return this.childNodes.filter(function (node) { return node.nodeType === 1; });
  });

  define(Element.prototype, "childElementCount", function () {
    return this.children.length;
  });

  define(Element.prototype, "innerHTML", function () {
    return this.__innerHTML || "";
  }, function (text) {
    this.__innerHTML = String(text);
    this.childNodes = [];
  });

  define(Element.prototype, "outerHTML", function () {
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
    var Ctor = tag(function () {
      throw new TypeError("Illegal constructor");
    }, constructorName, HTMLElement);
    elementKinds[name] = Ctor;
    return Ctor;
  }

  kind("div", "HTMLDivElement");
  kind("span", "HTMLSpanElement");
  kind("body", "HTMLBodyElement");
  kind("head", "HTMLHeadElement");
  kind("html", "HTMLHtmlElement");
  kind("a", "HTMLAnchorElement");
  kind("img", "HTMLImageElement");
  kind("script", "HTMLScriptElement");
  kind("iframe", "HTMLIFrameElement");
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
    elementKinds.video = HTMLMediaElement;
    elementKinds.audio = HTMLMediaElement;
  }

  HTMLFormElement.prototype.submit = function () {};
  HTMLFormElement.prototype.reset = function () {};

  HTMLInputElement.prototype.select = function () {};
  HTMLInputElement.prototype.setSelectionRange = function () {};

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
    return { data: new Uint8ClampedArray(count), width: width, height: height, colorSpace: "srgb" };
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
      if (!WebGLRenderingContext) return null;
      if (!this.__contextGl) {
        var context = Object.create(WebGLRenderingContext.prototype);
        context.canvas = this;
        context.drawingBufferWidth = this.width;
        context.drawingBufferHeight = this.height;
        Object.defineProperty(this, "__contextGl", { value: context, enumerable: false });
      }
      return this.__contextGl;
    }

    return null;
  };

  HTMLCanvasElement.prototype.toDataURL = function (type) {
    spend(12);
    var ops = this.__context2d ? drawing(this.__context2d) : "empty:" + this.width + "x" + this.height;
    return hostCanvas(fnv1a(ops), String(type || "image/png"), this.width, this.height);
  };

  HTMLCanvasElement.prototype.toBlob = function (callback, type) {
    var url = this.toDataURL(type);
    if (typeof callback === "function") callback({ size: url.length, type: type || "image/png" });
  };

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
    if (lower === "input" || lower === "textarea" || lower === "select") {
      node.value = "";
      node.type = lower === "input" ? "text" : lower;
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
  Document.prototype.elementFromPoint = function () { return document.body; };
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

  XMLHttpRequest.prototype.open = function (method, url) {
    this.__method = String(method);
    this.__url = String(url);
    this.readyState = 1;
    this.dispatchEvent(new Event("readystatechange"));
    if (typeof this.onreadystatechange === "function") this.onreadystatechange();
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

  XMLHttpRequest.prototype.send = function (body) {
    requestCount += 1;

    var answer = hostRequest({
      method: this.__method || "GET",
      url: this.__url || "",
      headers: this.__headers,
      body: body === undefined || body === null ? null : String(body),
      at: now(),
      source: "xhr"
    });

    this.__answer = answer;
    this.status = answer.status;
    this.statusText = answer.status === 200 ? "OK" : "";
    this.responseText = answer.body;
    this.response = this.responseType === "json" ? safeParse(answer.body) : answer.body;
    this.responseURL = this.__url || "";
    this.readyState = 4;

    this.dispatchEvent(new Event("readystatechange"));
    if (typeof this.onreadystatechange === "function") this.onreadystatechange();
    this.dispatchEvent(new ProgressEvent("load"));
    if (typeof this.onload === "function") this.onload();
    this.dispatchEvent(new ProgressEvent("loadend"));
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

    var answer = hostRequest({
      method: String(settings.method || "GET"),
      url: String(url),
      headers: headerPairs(settings.headers),
      body: settings.body === undefined || settings.body === null ? null : String(settings.body),
      at: now(),
      source: "fetch"
    });

    var headers = {};
    (answer.headers || []).forEach(function (pair) { headers[pair[0]] = pair[1]; });

    return Promise.resolve(new Response(answer.body, {
      status: answer.status,
      url: String(url),
      headers: headers
    }));
  }

  var Blob = tag(function Blob(parts, options) {
    var size = 0;
    (parts || []).forEach(function (part) { size += String(part).length; });
    this.size = size;
    this.type = (options && options.type) || "";
  }, "Blob");

  Blob.prototype.text = function () {
    return Promise.resolve("");
  };
  Blob.prototype.slice = function () {
    return this;
  };
  Blob.prototype.arrayBuffer = function () {
    return Promise.resolve(new ArrayBuffer(0));
  };

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
  URL.createObjectURL = function () {
    return "blob:" + page.location.origin + "/" + fnv1a(String(now()));
  };
  URL.revokeObjectURL = function () {};

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

  var Worker = tag(function Worker() {}, "Worker", EventTargetBase);
  Worker.prototype.postMessage = function () {};
  Worker.prototype.terminate = function () {};

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
  }, "AudioContext", EventTargetBase);

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
    return audioNode({
      fftSize: 2048,
      frequencyBinCount: 1024,
      getFloatFrequencyData: function () {},
      getByteFrequencyData: function () {}
    });
  };
  AudioContext.prototype.createGain = function () {
    return audioNode({ gain: param(1) });
  };
  AudioContext.prototype.createDynamicsCompressor = function () {
    return audioNode({
      threshold: param(-24), knee: param(30), ratio: param(12),
      attack: param(0.003), release: param(0.25), reduction: 0
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
        var seed = parseInt(rendered.slice(0, 8), 16) || 1;
        for (var index = 0; index < length; index += 1) {
          seed = (Math.imul(seed, 1664525) + 1013904223) >>> 0;
          data[index] = (seed / 4294967296) * 0.0002;
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

  Crypto.prototype.getRandomValues = function (target) {
    for (var index = 0; index < target.length; index += 1) {
      target[index] = Math.floor(Math.random() * 256);
    }
    return target;
  };

  Crypto.prototype.randomUUID = function () {
    var hex = "0123456789abcdef";
    var out = "";
    for (var index = 0; index < 36; index += 1) {
      if (index === 8 || index === 13 || index === 18 || index === 23) out += "-";
      else if (index === 14) out += "4";
      else out += hex.charAt(Math.floor(Math.random() * 16));
    }
    return out;
  };

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
        due: offset + Math.max(0, Number(delay) || 0),
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

  function runDue(until) {
    var ran = 0;

    for (var guard = 0; guard < 50000; guard += 1) {
      var next = null;

      for (var index = 0; index < timers.length; index += 1) {
        var timer = timers[index];
        if (timer.cancelled || timer.due > until) continue;
        if (!next || timer.due < next.due || (timer.due === next.due && timer.id < next.id)) {
          next = timer;
        }
      }

      if (!next) break;

      offset = Math.max(offset, next.due);

      if (next.every > 0) next.due = offset + next.every;
      else next.cancelled = true;

      try {
        next.fn.apply(globalThis, next.args);
        ran += 1;
      } catch (error) {
        void error;
      }
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

  if (profile.memory) {
    define(performance, "memory", function () {
      return {
        jsHeapSizeLimit: profile.memory.js_heap_size_limit,
        totalJSHeapSize: profile.memory.total_js_heap_size,
        usedJSHeapSize: profile.memory.used_js_heap_size
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

  function navigatorValue(name, entry) {
    Object.defineProperty(navigator, name, {
      value: entry,
      writable: false,
      enumerable: true,
      configurable: true
    });
  }

  navigatorValue("javaEnabled", function javaEnabled() { return false; });
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
    hostRequest({
      method: "POST",
      url: String(url),
      headers: {},
      body: body === undefined || body === null ? null : String(body),
      at: now(),
      source: "beacon"
    });
    return true;
  });

  if (profile.battery) {
    navigatorValue("getBattery", function getBattery() {
      return Promise.resolve({
        charging: profile.battery.charging,
        level: profile.battery.level,
        chargingTime: profile.battery.charging_time === null || profile.battery.charging_time === undefined
          ? Infinity
          : profile.battery.charging_time,
        dischargingTime: profile.battery.discharging_time === null || profile.battery.discharging_time === undefined
          ? Infinity
          : profile.battery.discharging_time,
        addEventListener: function () {},
        removeEventListener: function () {}
      });
    });
  }

  var mimeTypes = (profile.mime_types || []).map(function (entry) {
    return {
      type: entry.type,
      suffixes: entry.suffixes,
      description: entry.description,
      enabledPlugin: entry.plugin
    };
  });

  var mimeTypeArray = { length: mimeTypes.length };
  mimeTypes.forEach(function (entry, index) {
    mimeTypeArray[index] = entry;
    mimeTypeArray[entry.type] = entry;
  });
  mimeTypeArray.item = function (index) { return this[index] || null; };
  mimeTypeArray.namedItem = function (name) { return this[name] || null; };

  navigatorValue("mimeTypes", mimeTypeArray);

  if (profile.user_agent_data) {
    var brands = (profile.user_agent_data.brands || []).map(function (entry) {
      return { brand: entry.brand, version: entry.version };
    });

    var high = profile.user_agent_data.high_entropy || {};

    navigatorValue("userAgentData", {
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
    });
  }

  if (profile.connection) {
    navigatorValue("connection", {
      downlink: profile.connection.downlink,
      effectiveType: profile.connection.effective_type,
      rtt: profile.connection.rtt,
      saveData: Boolean(profile.connection.save_data),
      onchange: null,
      addEventListener: function () {},
      removeEventListener: function () {}
    });
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

  navigatorValue("mediaDevices", {
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
  });

  navigatorValue("storage", {
    estimate: function () {
      return Promise.resolve({ quota: 299977904946, usage: 0, usageDetails: {} });
    },
    persisted: function () { return Promise.resolve(false); }
  });

  navigatorValue("serviceWorker", {
    controller: null,
    ready: new Promise(function () {}),
    register: function () { return Promise.reject(new Error("not supported")); },
    getRegistration: function () { return Promise.resolve(undefined); },
    getRegistrations: function () { return Promise.resolve([]); },
    addEventListener: function () {},
    removeEventListener: function () {}
  });

  navigatorValue("credentials", {
    get: function () { return Promise.resolve(null); },
    store: function () { return Promise.resolve(); },
    preventSilentAccess: function () { return Promise.resolve(); }
  });

  navigatorValue("clipboard", {
    readText: function () { return Promise.reject(new Error("Read permission denied.")); },
    writeText: function () { return Promise.resolve(); }
  });

  navigatorValue("bluetooth", {
    getAvailability: function () { return Promise.resolve(false); },
    requestDevice: function () { return Promise.reject(new Error("Bluetooth adapter not available.")); }
  });

  navigatorValue("wakeLock", { request: function () { return Promise.reject(new Error("not allowed")); } });
  navigatorValue("locks", { query: function () { return Promise.resolve({ held: [], pending: [] }); } });
  navigatorValue("webkitTemporaryStorage", { queryUsageAndQuota: function () {} });
  navigatorValue("webkitPersistentStorage", { queryUsageAndQuota: function () {} });
  navigatorValue("scheduling", { isInputPending: function () { return false; } });
  navigatorValue("userActivation", { hasBeenActive: true, isActive: false });

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
      return offsetMinutes;
    };
    masked.push("Date.prototype.getTimezoneOffset");
  }

  var chromeShape = profile.chrome;

  if (chromeShape) {
    globalThis.chrome = {
      runtime: chromeShape.runtime || {},
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
  crypto.subtle = {
    digest: function () { return Promise.reject(new Error("not supported")); }
  };

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
  globalThis.indexedDB = {
    open: function () {
      var request = { result: null, onsuccess: null, onerror: null,
        addEventListener: function () {}, removeEventListener: function () {} };
      return request;
    },
    databases: function () { return Promise.resolve([]); },
    deleteDatabase: function () { return { onsuccess: null, onerror: null }; }
  };
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
  globalThis.scrollTo = function scrollTo() {};
  globalThis.scrollBy = function scrollBy() {};
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

  function build(spec) {
    document.URL = spec.location.href;
    document.documentURI = spec.location.href;
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

    html.clientWidth = globalThis.innerWidth;
    html.clientHeight = globalThis.innerHeight;
    html.offsetWidth = globalThis.innerWidth;
    html.offsetHeight = globalThis.innerHeight;
    body.clientWidth = globalThis.innerWidth;
    body.clientHeight = globalThis.innerHeight;
    body.offsetWidth = globalThis.innerWidth;
    body.offsetHeight = globalThis.innerHeight;
    body.offsetParent = null;
    html.__innerHTML = spec.html || "";

    var scripts = (spec.scripts || []).map(function (src) {
      var node = element("script");
      if (src && src !== "[inline]") attribute(node, "src", src);
      node.async = false;
      node.defer = false;
      head.appendChild(node);
      return node;
    });

    document.scripts = scripts;
    document.currentScript = scripts.length ? scripts[scripts.length - 1] : null;

    var forms = (spec.forms || []).map(function (entry) {
      var node = element("form");
      Object.keys(entry.attributes || {}).forEach(function (name) {
        attribute(node, name, entry.attributes[name]);
      });
      body.appendChild(node);
      return node;
    });

    (spec.inputs || []).forEach(function (entry) {
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
    });

    document.forms = forms;
    document.images = [];
    document.links = [];
    document.all = descendants(document);
    document.embeds = [];
    document.plugins = [];
    document.styleSheets = [];
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
      offset += cost;
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
    else made = new Event(type, options);

    made.isTrusted = true;
    made.timeStamp = elapsed();

    var target = options.target === "window" ? globalThis : document.body;
    made.target = target;
    made.srcElement = target;

    if (target !== globalThis) {
      target.dispatchEvent(made);
      document.dispatchEvent(made);
      document.documentElement.dispatchEvent(made);
    }

    globalThis.dispatchEvent(made);
    return true;
  }

  function keyDetail(options) {
    var key = String(options.key || "");
    var code = options.keyCode;

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
  masked.push("performance");
  masked.push("Performance.prototype");
  masked.push("Storage.prototype");
  masked.push("Location.prototype");
  masked.push("History.prototype");
  masked.push("Crypto.prototype");
  masked.push("CanvasRenderingContext2D.prototype");

  delete globalThis.__wreProfileBlob;
  delete globalThis.__wrePageBlob;
  delete globalThis.__wreRequest;
  delete globalThis.__wreCookieRead;
  delete globalThis.__wreCookieWrite;
  delete globalThis.__wreCanvasImage;
  delete globalThis.__wreMeasureText;
  delete globalThis.__wreMiss;
  delete globalThis.__wreRealNow;

  return {
    advance: function (ms) {
      var until = offset + Math.max(0, Number(ms) || 0);
      var ran = runDue(until);
      offset = Math.max(offset, until);
      return ran;
    },
    settle: function (rounds) {
      var ran = 0;
      for (var round = 0; round < (rounds || 8); round += 1) {
        var did = runDue(offset);
        ran += did;
        if (!did) break;
      }
      return ran;
    },
    fire: fire,
    ready: function (state) {
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
    friction: function (ms) {
      friction = Math.max(0, Number(ms) || 0);
      return friction;
    },
    jump: function (ms) {
      offset += Math.max(0, Number(ms) || 0);
      return offset;
    },
    masked: function () {
      return masked.slice();
    },
    cookies: function () {
      return hostCookieRead();
    }
  };
})()
