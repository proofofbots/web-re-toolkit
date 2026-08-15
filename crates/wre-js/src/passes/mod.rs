pub mod bindings;
pub mod simplify;
pub mod structure;
pub mod tables;
pub mod unflatten;
pub mod wrappers;

use crate::naming::rename_identifiers;
use crate::pipeline::{PassSpec, Pipeline};

pub const REGISTRY: &[PassSpec] = &[
    PassSpec {
        name: "simplify-literals",
        description: "!0 and !1 back to booleans, void 0 to undefined, un-Yoda comparisons",
        needs_scope: false,
        run: simplify::simplify_literals,
    },
    PassSpec {
        name: "ensure-blocks",
        description: "give every branch and loop a block so statements can be spliced in",
        needs_scope: false,
        run: structure::ensure_blocks,
    },
    PassSpec {
        name: "unflatten-switch-order",
        description: "put a shuffled while-switch dispatch back into source order",
        needs_scope: false,
        run: unflatten::unflatten_switch_order,
    },
    PassSpec {
        name: "split-declarations",
        description: "one binding per statement",
        needs_scope: false,
        run: structure::split_declarations,
    },
    PassSpec {
        name: "inline-operator-wrappers",
        description: "fold the javascript-obfuscator operator wrapper functions",
        needs_scope: false,
        run: wrappers::inline_operator_wrappers,
    },
    PassSpec {
        name: "inline-wrapper-objects",
        description: "fold the dispatch-table form of the operator wrappers",
        needs_scope: false,
        run: wrappers::inline_wrapper_objects,
    },
    PassSpec {
        name: "remove-wrapper-definitions",
        description: "drop wrapper definitions once every call site is folded",
        needs_scope: false,
        run: wrappers::remove_wrapper_definitions,
    },
    PassSpec {
        name: "apply-call-table",
        description: "replace recorded decoder calls with the value they produced",
        needs_scope: false,
        run: tables::apply_call_table,
    },
    PassSpec {
        name: "inline-index-tables",
        description: "replace reads of a recorded table with the value at that index",
        needs_scope: false,
        run: tables::inline_index_tables,
    },
    PassSpec {
        name: "resolve-hash-arguments",
        description: "replace hash constants with the names they stand for",
        needs_scope: false,
        run: tables::resolve_hash_arguments,
    },
    PassSpec {
        name: "restore-member-reads",
        description: "readProp(o, \"f\") back to o.f",
        needs_scope: false,
        run: tables::restore_member_reads,
    },
    PassSpec {
        name: "inline-constant-bindings",
        description: "push literal-valued bindings to their use sites",
        needs_scope: true,
        run: bindings::inline_constant_bindings,
    },
    PassSpec {
        name: "fold-constants",
        description: "evaluate the arithmetic and concatenation that inlining exposes",
        needs_scope: false,
        run: simplify::fold_constants,
    },
    PassSpec {
        name: "decode-base64-literals",
        description: "fold atob on a constant",
        needs_scope: false,
        run: simplify::decode_base64_literals,
    },
    PassSpec {
        name: "inline-global-aliases",
        description: "var t = atob back to atob at every use site",
        needs_scope: true,
        run: bindings::inline_global_aliases,
    },
    PassSpec {
        name: "restore-nullish-operators",
        description: "rebuild ?? from the downlevelled ternaries",
        needs_scope: false,
        run: simplify::restore_nullish_operators,
    },
    PassSpec {
        name: "normalize-member-access",
        description: "o[\"name\"] to o.name",
        needs_scope: false,
        run: simplify::normalize_member_access,
    },
    PassSpec {
        name: "normalize-object-keys",
        description: "{ [\"s94\"]: fn } to { s94: fn }",
        needs_scope: false,
        run: simplify::normalize_object_keys,
    },
    PassSpec {
        name: "flatten-sequences",
        description: "(a(), b(), c) to three statements",
        needs_scope: false,
        run: structure::flatten_sequences,
    },
    PassSpec {
        name: "expand-return-sequences",
        description: "return a(), b to a call then a return",
        needs_scope: false,
        run: structure::expand_return_sequences,
    },
    PassSpec {
        name: "statementize-control-flow",
        description: "a && b() to if (a) b()",
        needs_scope: false,
        run: structure::statementize_control_flow,
    },
    PassSpec {
        name: "statementize-returns",
        description: "return a ? x : y to the if chain",
        needs_scope: false,
        run: structure::statementize_returns,
    },
    PassSpec {
        name: "merge-nested-blocks",
        description: "drop blocks that declare nothing",
        needs_scope: false,
        run: structure::merge_nested_blocks,
    },
    PassSpec {
        name: "drop-debugger",
        description: "remove debugger statements",
        needs_scope: false,
        run: simplify::drop_debugger,
    },
    PassSpec {
        name: "remove-unused-bindings",
        description: "drop what the other passes orphaned",
        needs_scope: true,
        run: bindings::remove_unused_bindings,
    },
    PassSpec {
        name: "rename-identifiers",
        description: "one readable name per binding, from evidence",
        needs_scope: true,
        run: rename_identifiers,
    },
];

pub fn standard_pipeline() -> Pipeline {
    Pipeline::new(REGISTRY.to_vec())
}

pub fn pipeline_named(names: &[&str]) -> Pipeline {
    Pipeline::new(
        REGISTRY
            .iter()
            .copied()
            .filter(|pass| names.contains(&pass.name))
            .collect(),
    )
}

pub fn find(name: &str) -> Option<PassSpec> {
    REGISTRY.iter().copied().find(|pass| pass.name == name)
}

pub fn names() -> Vec<&'static str> {
    REGISTRY.iter().map(|pass| pass.name).collect()
}
