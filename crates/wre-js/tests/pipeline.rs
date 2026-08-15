use std::collections::HashMap;

use wre_js::eval::Const;
use wre_js::pipeline::{Config, MemberReadSpec, RenameConfig};
use wre_js::{standard_pipeline, pipeline_named};

fn run(source: &str, config: Config) -> String {
    standard_pipeline()
        .run(source, config)
        .expect("pipeline ran")
        .code
}

fn structural(source: &str) -> String {
    run(source, Config::structural())
}

#[test]
fn simplifies_obfuscator_literals() {
    let out = structural("var a = !0; var b = !1; var c = void 0; if ('x' === q) { f(); }");
    assert!(out.contains("var a = true;"), "{out}");
    assert!(out.contains("var b = false;"), "{out}");
    assert!(out.contains("var c = undefined;"), "{out}");
    assert!(out.contains("q === 'x'") || out.contains("q === \"x\""), "{out}");
}

#[test]
fn folds_operator_wrappers() {
    let source = r#"
        function _0x11(a, b) { return a + b; }
        function _0x22(a, b) { return a * b; }
        var total = _0x11(price, tax);
        var area = _0x22(width, height);
    "#;

    let out = structural(source);
    assert!(out.contains("price + tax"), "{out}");
    assert!(out.contains("width * height"), "{out}");
    assert!(!out.contains("_0x11("), "{out}");
}

#[test]
fn folds_wrapper_objects() {
    let source = r#"
        var _0xtab = {
            'aBc': function (a, b) { return a + b; },
            'dEf': function (a, b) { return a === b; }
        };
        var one = _0xtab['aBc'](left, right);
        var two = _0xtab.dEf(left, right);
    "#;

    let out = structural(source);
    assert!(out.contains("left + right"), "{out}");
    assert!(out.contains("left === right"), "{out}");
}

#[test]
fn statementizes_control_flow_and_returns() {
    let source = r#"
        function pick(a, b) {
            ready && start();
            failed || retry();
            return a ? b : c;
        }
    "#;

    let out = structural(source);
    assert!(out.contains("if (ready)"), "{out}");
    assert!(out.contains("if (!failed)"), "{out}");
    assert!(out.contains("return b;"), "{out}");
    assert!(out.contains("return c;"), "{out}");
}

#[test]
fn applies_a_recorded_call_table() {
    let mut config = Config::structural();
    config.call_values.insert(
        "_0xdec(n:5,s:salt)".to_string(),
        Const::Text("canvas".to_string()),
    );
    config
        .thunk_values
        .insert("_0xthunk".to_string(), Const::Text("webgl".to_string()));

    let out = run(
        "var a = _0xdec(5, 'salt'); var b = _0xthunk(1, 2, 3); var c = _0xdec(9, 'x');",
        config,
    );

    assert!(out.contains("'canvas'") || out.contains("\"canvas\""), "{out}");
    assert!(out.contains("'webgl'") || out.contains("\"webgl\""), "{out}");
    assert!(out.contains("_0xdec(9"), "{out}");
}

#[test]
fn inlines_index_tables() {
    let mut config = Config::structural();
    config.index_tables.insert(
        "vault".to_string(),
        vec![
            Const::Text("alpha".to_string()),
            Const::Text("beta".to_string()),
        ],
    );

    let out = run("send(vault[0], vault[1], vault[7]);", config);
    assert!(out.contains("alpha"), "{out}");
    assert!(out.contains("beta"), "{out}");
    assert!(out.contains("vault[7]"), "{out}");
}

#[test]
fn restores_member_reads() {
    let mut config = Config::structural();
    config.member_reads.push(MemberReadSpec {
        function: "readProp".to_string(),
        object_arg: 0,
        key_arg: 1,
    });

    let out = run("readProp(target, 'getContext')(2);", config);
    assert!(out.contains("target.getContext(2)"), "{out}");
}

#[test]
fn resolves_hash_arguments() {
    let mut config = Config::structural();
    config.hash_functions.push("findName".to_string());
    config.hash_names.insert(3405691582, "deviceMemory".to_string());

    let out = run("var x = findName(0xcafebabe);", config);
    assert!(out.contains("deviceMemory"), "{out}");
}

#[test]
fn inlines_constants_and_folds() {
    let source = "var pad = 3; var head = 'x'; var out = head + (pad * 2);";
    let out = structural(source);
    assert!(out.contains("'x6'") || out.contains("\"x6\""), "{out}");
}

#[test]
fn inlines_bare_global_aliases() {
    let mut config = Config::structural();
    config.inline_global_aliases = true;

    let out = run("var t = atob; var v = t('QQ=='); var u = t('Qg==');", config);
    assert!(!out.contains("var t = atob"), "{out}");
    assert!(out.contains("'A'") || out.contains("\"A\""), "{out}");
}

#[test]
fn leaves_member_path_aliases_alone() {
    let mut config = Config::structural();
    config.inline_global_aliases = true;

    let out = run("var t = Object.keys; var v = t(o);", config);
    assert!(out.contains("Object.keys"), "{out}");
    assert!(out.contains("t(o)"), "{out}");
}

#[test]
fn restores_nullish_coalescing() {
    let out = structural("var v = a === null || a === void 0 ? fallback : a;");
    assert!(out.contains("??"), "{out}");
}

#[test]
fn normalizes_member_and_key_syntax() {
    let out = structural("var o = { ['s94']: 1 }; o['s94'] = o['not a name'];");
    assert!(out.contains("s94: 1"), "{out}");
    assert!(out.contains("o.s94 ="), "{out}");
    assert!(out.contains("o['not a name']") || out.contains("o[\"not a name\"]"), "{out}");
}

#[test]
fn renames_junk_identifiers_from_evidence() {
    let mut config = Config::readable();
    config.remove_unused = false;
    config.rename = RenameConfig { enabled: true, infer: true, ..RenameConfig::default() };

    let source = r#"
        function _0xdeadbeef() {
            var _0x1 = document.createElement('canvas');
            var _0x2 = _0x1.getContext('webgl');
            return _0x2;
        }
    "#;

    let out = run(source, config);
    assert!(!out.contains("_0x1"), "{out}");
    assert!(out.contains("createelement") || out.contains("Createelement"), "{out}");
}

#[test]
fn rename_without_inference_is_deterministic() {
    let mut config = Config::readable();
    config.remove_unused = false;
    config.rename = RenameConfig { enabled: true, infer: false, ..RenameConfig::default() };

    let out = run("function _0xaa(_0xbb) { var _0xcc = _0xbb + 1; return _0xcc; }", config);
    assert!(out.contains("fn"), "{out}");
    assert!(!out.contains("_0xcc"), "{out}");
}

#[test]
fn keeps_meaningful_names() {
    let mut config = Config::readable();
    config.remove_unused = false;
    config.rename.enabled = true;

    let out = run("function deriveKey(salt) { var canvasHash = salt; return canvasHash; }", config);
    assert!(out.contains("deriveKey"), "{out}");
    assert!(out.contains("canvasHash"), "{out}");
}

#[test]
fn removes_orphaned_bindings_only_when_asked() {
    let kept = structural("var unused = 5; used();");
    assert!(kept.contains("var unused = 5"), "{kept}");

    let mut config = Config::structural();
    config.remove_unused = true;
    let dropped = run("var unused = 5; var alsoUnused = 'x'; used();", config);
    assert!(!dropped.contains("unused"), "{dropped}");
    assert!(dropped.contains("used()"), "{dropped}");
}

#[test]
fn keeps_bindings_with_side_effecting_initialisers() {
    let mut config = Config::structural();
    config.remove_unused = true;
    let out = run("var ignored = start(); done();", config);
    assert!(out.contains("start()"), "{out}");
}

#[test]
fn preserves_evaluation_order_for_impure_wrapper_arguments() {
    let source = r#"
        function _0xswap(a, b) { return b + a; }
        var v = _0xswap(first(), second());
    "#;

    let out = structural(source);
    assert!(out.contains("_0xswap(first(), second())"), "{out}");
}

#[test]
fn folds_reordered_wrappers_when_arguments_are_pure() {
    let source = r#"
        function _0xswap(a, b) { return b + a; }
        var v = _0xswap(left, right);
    "#;

    let out = structural(source);
    assert!(out.contains("right + left"), "{out}");
}

#[test]
fn reports_per_pass_statistics() {
    let outcome = standard_pipeline()
        .run("var a = !0; var b = !1;", Config::structural())
        .expect("pipeline ran");

    let changes = outcome.changes_by_pass();
    assert!(changes.get("simplify-literals").copied().unwrap_or(0) >= 2);
    assert!(outcome.converged);
}

#[test]
fn a_named_subset_runs_only_those_passes() {
    let out = pipeline_named(&["simplify-literals"])
        .run("var a = !0; (f(), g());", Config::structural())
        .expect("pipeline ran")
        .code;

    assert!(out.contains("var a = true;"), "{out}");
    assert!(out.contains("f(), g()"), "{out}");
}

#[test]
fn drops_debugger_only_when_asked() {
    let mut config = Config::structural();
    let kept = run("debugger; work();", config.clone());
    assert!(kept.contains("debugger"), "{kept}");

    config.drop_debugger = true;
    let dropped = run("debugger; work();", config);
    assert!(!dropped.contains("debugger"), "{dropped}");
}

#[test]
fn survives_a_realistic_obfuscated_shape() {
    let source = r#"
        var _0x3f = {
            'kQx': function (a, b) { return a + b; },
            'wPz': function (a, b) { return a === b; }
        };
        function _0xmain(_0xarg) {
            var _0xel = document['createElement']('canvas');
            var _0xctx = _0xel['getContext']('2d');
            _0xctx && _0xctx['fillRect'](0, 0, !0 ? 1 : 2, 3);
            var _0xname = _0x3f['kQx']('ca', 'nvas');
            return _0x3f['wPz'](_0xname, _0xarg) ? _0xctx : null;
        }
    "#;

    let mut config = Config::readable();
    config.remove_unused = false;
    config.max_sweeps = 8;

    let outcome = standard_pipeline().run(source, config).expect("pipeline ran");
    let out = outcome.code;

    assert!(!out.contains("_0x3f"), "{out}");
    assert!(out.contains("'canvas'") || out.contains("\"canvas\""), "{out}");
    assert!(out.contains(".getContext("), "{out}");
    assert!(out.contains(".fillRect("), "{out}");
    assert!(outcome.converged, "did not converge: {:?}", outcome.sweeps.len());
}

#[test]
fn output_stays_parseable() {
    let source = r#"
        var _0xa = { 'x': function (p, q) { return p | q; } };
        for (var i = 0; i < 4; i++) if (i) _0xa['x'](i, 1); else k(i);
        try { risky(); } catch (e) { report(e); } finally { done(); }
        class Thing { constructor() { this.v = 1; } get value() { return this.v; } }
        const arrow = (a, b) => a ?? b;
        label: while (true) { break label; }
    "#;

    let out = structural(source);
    let errors = wre_js::parse_errors(&out, wre_js::SourceKind::Script);
    assert!(errors.is_empty(), "reparse failed: {errors:?}\n{out}");
}

#[test]
fn table_key_format_is_stable() {
    let key = wre_js::passes::tables::call_key(
        "dec",
        &[Const::Number(5.0), Const::Text("salt".into()), Const::Bool(true)],
    );
    assert_eq!(key, "dec(n:5,s:salt,b:true)");
}

#[test]
fn config_is_cloneable_for_repeated_runs() {
    let mut config = Config::readable();
    config.remove_unused = false;
    let mut table: HashMap<String, Const> = HashMap::new();
    table.insert("f()".into(), Const::Number(1.0));

    let mut second = config.clone();
    second.call_values = table;

    assert!(run("var counter = 1;", config).contains("var counter = 1"));
    assert!(run("var total = f();", second).contains("var total = 1"));
}
