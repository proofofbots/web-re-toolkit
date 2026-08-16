(function () {
  var env = globalThis.__ENV;
  var target = Number(globalThis.__CLOCK);

  delete globalThis.__CLOCK;

  if (!isFinite(target)) return;

  var RealDate = Date;
  var realNow = RealDate.now;
  var shift = target - realNow.call(RealDate);

  var now = function now() {
    return realNow.call(RealDate) + shift;
  };

  env.sources.set(now, env.nativeSource("now"));

  var shifted = new Proxy(RealDate, {
    construct: function (holder, argv, newTarget) {
      if (argv.length === 0) return Reflect.construct(holder, [now()], newTarget);
      return Reflect.construct(holder, argv, newTarget);
    },
    apply: function () {
      return RealDate.prototype.toString.call(new RealDate(now()));
    },
    get: function (holder, key, receiver) {
      if (key === "now") return now;
      return Reflect.get(holder, key, receiver);
    },
  });

  Object.defineProperty(globalThis, "Date", { value: shifted, writable: true, enumerable: false, configurable: true });
})();
