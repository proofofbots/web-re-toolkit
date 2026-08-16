(function () {
  var env = globalThis.__ENV;
  var pump = globalThis.__PUMP;

  delete globalThis.__ENV;
  delete globalThis.__PUMP;

  var json = function (value) {
    try {
      return JSON.parse(env.rawStringify(value));
    } catch (error) {
      return null;
    }
  };

  return {
    step: function () {
      return pump.step(env);
    },

    pending: function () {
      return pump.pending();
    },

    begin: function (name, inline) {
      env.beginScript(String(name || ""), Boolean(inline));
    },

    end: function () {
      env.endScript();
    },

    read: function (expression) {
      try {
        return env.evaluate("(" + String(expression) + ")");
      } catch (error) {
        return "THREW " + String(error && error.message);
      }
    },

    cookies: function () {
      return typeof env.cookies === "function" ? env.cookies() : "";
    },

    calls: function () {
      return json(env.calls);
    },

    trail: function () {
      return json(env.trail);
    },

    misses: function () {
      return json(env.misses);
    },

    log: function () {
      return json(env.log);
    },

    layoutMisses: function () {
      return typeof env.layoutMisses === "function" ? json(env.layoutMisses()) : [];
    }
  };
})()
