use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use wre_core::error::{Error, Result};
use wre_live::realm::{Realm, RealmOptions};

use crate::ir::OpKind;

pub const KERNEL: &str = r#"
globalThis.__vmKernel = (function () {
  function makeSentinel(tag, recorder) {
    var carrier = function () {};
    carrier.__vmTag = tag;

    var proxy = new Proxy(carrier, {
      get: function (target, property) {
        if (property === Symbol.toPrimitive) {
          return function (hint) { return hint === "number" ? 0 : ""; };
        }
        if (property === Symbol.iterator) return undefined;
        if (property === "length") return 0;
        if (property === "constructor") return Object;
        if (property === "__vmTag") return tag;
        if (typeof property === "symbol") return undefined;
        recorder.touches.push({ tag: tag, key: String(property) });
        return proxy;
      },
      set: function (target, property, value) {
        if (typeof property !== "symbol") {
          recorder.writes.push({ tag: tag, key: String(property) });
        }
        return true;
      },
      has: function () { return true; },
      apply: function () { recorder.calls.push({ tag: tag }); return proxy; },
      construct: function () { recorder.calls.push({ tag: tag, construct: true }); return proxy; },
      deleteProperty: function () { return true; },
      ownKeys: function () { return []; },
      getOwnPropertyDescriptor: function () { return undefined; }
    });

    return proxy;
  }

  function newRecorder(options) {
    var recorder = {
      reads: [],
      writes: [],
      jumps: [],
      calls: [],
      touches: [],
      error: null,
      options: options || {},
      readCount: 0
    };

    recorder.sentinel = function (tag) { return makeSentinel(tag || "s" + recorder.touches.length, recorder); };

    recorder.read = function (value) {
      recorder.readCount++;
      recorder.reads.push(value === undefined ? null : value);
      if (recorder.options.falsyRead && recorder.readCount === recorder.options.falsyRead) return 0;
      return recorder.sentinel("read" + recorder.readCount);
    };

    recorder.write = function (slot) { recorder.writes.push({ slot: slot }); };
    recorder.jump = function (target) { recorder.jumps.push({ at: recorder.reads.length - 1, target: target }); };
    recorder.call = function (info) { recorder.calls.push(info || {}); };

    return recorder;
  }

  function probe(handler, makeArgs, options) {
    var recorder = newRecorder(options);
    try {
      var args = makeArgs(recorder, recorder.options);
      handler.apply(recorder.options.receiver === undefined ? null : recorder.options.receiver, args);
    } catch (error) {
      recorder.error = String((error && error.message) || error);
    }
    return {
      reads: recorder.reads,
      writes: recorder.writes,
      jumps: recorder.jumps,
      calls: recorder.calls,
      touches: recorder.touches.slice(0, 64),
      error: recorder.error
    };
  }

  return { probe: probe, sentinel: makeSentinel };
})();
"#;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FrameModel {
    pub make_args: String,
    #[serde(default)]
    pub arity: usize,
    #[serde(default)]
    pub receiver: Option<Value>,
}

impl FrameModel {
    pub fn new(make_args: impl Into<String>) -> Self {
        Self { make_args: make_args.into(), arity: 0, receiver: None }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ProbeRecord {
    #[serde(default)]
    pub reads: Vec<Value>,
    #[serde(default)]
    pub writes: Vec<Value>,
    #[serde(default)]
    pub jumps: Vec<Value>,
    #[serde(default)]
    pub calls: Vec<Value>,
    #[serde(default)]
    pub touches: Vec<Value>,
    #[serde(default)]
    pub error: Option<String>,
}

impl ProbeRecord {
    pub fn slots_written(&self) -> Vec<u32> {
        self.writes
            .iter()
            .filter_map(|entry| entry.get("slot").and_then(Value::as_u64))
            .map(|slot| slot as u32)
            .collect()
    }

    pub fn jump_targets(&self) -> Vec<Value> {
        self.jumps
            .iter()
            .filter_map(|entry| entry.get("target").cloned())
            .collect()
    }

    pub fn touched_keys(&self) -> Vec<String> {
        self.touches
            .iter()
            .filter_map(|entry| entry.get("key").and_then(Value::as_str).map(str::to_string))
            .collect()
    }

    pub fn failed(&self) -> bool {
        self.error.is_some()
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct HandlerProfile {
    pub index: usize,
    pub straight: ProbeRecord,
    #[serde(default)]
    pub alternate: Option<ProbeRecord>,
    pub conditional: bool,
    pub reads: usize,
    pub writes: usize,
    pub jumps: usize,
    pub calls: usize,
    #[serde(default)]
    pub kind: OpKind,
    #[serde(default)]
    pub source: Option<String>,
}

pub struct Prober {
    realm: Realm,
    model: Option<FrameModel>,
}

impl Prober {
    pub fn from_realm(mut realm: Realm) -> Result<Self> {
        realm.eval_unit(KERNEL, "wre:vm-kernel")?;
        Ok(Self { realm, model: None })
    }

    pub fn fresh(options: RealmOptions) -> Result<Self> {
        Self::from_realm(Realm::new(options)?)
    }

    pub fn realm(&mut self) -> &mut Realm {
        &mut self.realm
    }

    pub fn install(&mut self, model: FrameModel) -> Result<()> {
        self.realm
            .eval_unit(
                &format!("globalThis.__vmMakeArgs = ({});", model.make_args),
                "wre:vm-frame",
            )
            .map_err(|error| Error::msg(format!("frame model did not install: {error}")))?;
        self.model = Some(model);
        Ok(())
    }

    pub fn probe(&mut self, handler: &str, options: &Value) -> Result<ProbeRecord> {
        if self.model.is_none() {
            return Err(Error::msg("no frame model installed"));
        }

        let expression = format!(
            "__vmKernel.probe({handler}, __vmMakeArgs, {})",
            serde_json::to_string(options).unwrap_or_else(|_| "{}".to_string())
        );

        let raw = self.realm.eval_json(&expression)?;
        serde_json::from_value(raw)
            .map_err(|error| Error::msg(format!("probe record did not parse: {error}")))
    }

    pub fn profile(&mut self, handler: &str, index: usize) -> Result<HandlerProfile> {
        let straight = self.probe(handler, &json!({}))?;
        let alternate = self.probe(handler, &json!({ "falsyRead": 1 })).ok();

        let conditional = alternate
            .as_ref()
            .map(|other| {
                !other.failed()
                    && (other.jumps.len() != straight.jumps.len()
                        || other.reads.len() != straight.reads.len()
                        || other.writes.len() != straight.writes.len())
            })
            .unwrap_or(false);

        let source = self
            .realm
            .eval_json(&format!("String({handler})"))
            .ok()
            .and_then(|value| value.as_str().map(str::to_string));

        let kind = source
            .as_deref()
            .map(classify_source)
            .unwrap_or(OpKind::Unknown);

        Ok(HandlerProfile {
            index,
            reads: straight.reads.len(),
            writes: straight.writes.len(),
            jumps: straight.jumps.len(),
            calls: straight.calls.len(),
            conditional,
            straight,
            alternate,
            kind,
            source,
        })
    }

    pub fn profile_table(&mut self, table: &str, limit: usize) -> Result<Vec<HandlerProfile>> {
        let length = self
            .realm
            .eval_json(&format!("({table}).length"))?
            .as_u64()
            .unwrap_or(0) as usize;

        let count = if limit == 0 { length } else { length.min(limit) };
        let mut out = Vec::with_capacity(count);

        for index in 0..count {
            let handler = format!("({table})[{index}]");

            let is_function = self
                .realm
                .eval_json(&format!("typeof {handler} === 'function'"))?
                .as_bool()
                .unwrap_or(false);

            if !is_function {
                continue;
            }

            match self.profile(&handler, index) {
                Ok(profile) => out.push(profile),
                Err(error) => {
                    tracing::debug!("handler {index} did not profile: {error}");
                }
            }
        }

        Ok(out)
    }

    pub fn handler_sources(&mut self, table: &str, limit: usize) -> Result<Vec<String>> {
        let expression = format!(
            "Array.prototype.slice.call({table}, 0, {limit}).map(function (entry) {{ return typeof entry === 'function' ? String(entry) : ''; }})"
        );

        let value = self.realm.eval_json(&expression)?;
        Ok(value
            .as_array()
            .map(|items| {
                items
                    .iter()
                    .map(|item| item.as_str().unwrap_or_default().to_string())
                    .collect()
            })
            .unwrap_or_default())
    }
}

pub fn classify_source(source: &str) -> OpKind {
    let compact: String = source.split_whitespace().collect::<Vec<_>>().join(" ");

    if compact.contains("throw ") {
        return OpKind::Throw;
    }
    if compact.contains("return null") && compact.len() < 80 {
        return OpKind::Halt;
    }
    if compact.contains(".apply(") || compact.contains(".call(") {
        return OpKind::Call;
    }
    if compact.contains("new ") {
        return OpKind::New;
    }

    for operator in [
        "===", "!==", ">>>", "<<", ">>", "==", "!=", "<=", ">=", "&&", "||", "+", "-", "*", "/",
        "%", "&", "|", "^",
    ] {
        let needle = format!(" {operator} ");
        if compact.contains(&needle) {
            return OpKind::Binary { operator: operator.to_string() };
        }
    }

    if compact.contains("typeof ") {
        return OpKind::Unary { operator: "typeof ".to_string() };
    }
    if compact.contains("!") {
        return OpKind::Unary { operator: "!".to_string() };
    }

    OpKind::Unknown
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classifies_handler_sources() {
        assert_eq!(
            classify_source("function (n, e) { return e(n) + e(n); }"),
            OpKind::Binary { operator: "+".to_string() }
        );
        assert!(matches!(classify_source("function () { throw x; }"), OpKind::Throw));
        assert!(matches!(classify_source("function () { return null; }"), OpKind::Halt));
        assert!(matches!(
            classify_source("function (n, e) { return n.f.apply(n, e); }"),
            OpKind::Call
        ));
    }
}
