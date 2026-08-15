use oxc_allocator::GetAllocator;
use std::collections::{BTreeMap, HashMap, HashSet};

use oxc_ast::ast::*;
use oxc_ast_visit::{Visit, VisitMut, walk, walk_mut};
use oxc_semantic::{Scoping, SymbolId};

use crate::passes::simplify::is_identifier_name;
use crate::pipeline::PassContext;

pub const UNINFORMATIVE_PROPERTIES: [&str; 46] = [
    "length", "call", "apply", "bind", "prototype", "constructor", "toString", "valueOf", "then",
    "catch", "finally", "push", "pop", "shift", "unshift", "slice", "splice", "concat", "join",
    "map", "filter", "reduce", "forEach", "indexOf", "charCodeAt", "charAt", "split", "replace",
    "test", "exec", "keys", "values", "entries", "hasOwnProperty", "value", "type", "name", "data",
    "result", "state", "status", "code", "index", "item", "key", "id",
];

pub const GENERIC_CALLS: [&str; 12] = [
    "call", "apply", "bind", "toString", "valueOf", "then", "map", "filter", "forEach", "push",
    "concat", "slice",
];

pub const RESERVED: [&str; 41] = [
    "arguments", "await", "break", "case", "catch", "class", "const", "continue", "debugger",
    "default", "delete", "do", "else", "enum", "eval", "export", "extends", "false", "finally",
    "for", "function", "if", "implements", "import", "in", "instanceof", "let", "new", "null",
    "package", "return", "static", "super", "switch", "this", "throw", "true", "try", "typeof",
    "var", "void",
];

pub fn is_junk_name(name: &str) -> bool {
    if name.len() <= 2 {
        return true;
    }

    let lower = name.to_ascii_lowercase();
    let trimmed = lower.trim_start_matches('_');

    if let Some(rest) = trimmed.strip_prefix("0x") {
        if !rest.is_empty() && rest.chars().all(|ch| ch.is_ascii_hexdigit()) {
            return true;
        }
    }

    if trimmed.chars().all(|ch| ch.is_ascii_digit()) && !trimmed.is_empty() {
        return true;
    }

    let mut chars = trimmed.chars();
    if let Some(first) = chars.next() {
        if first.is_ascii_alphabetic() && chars.all(|ch| ch.is_ascii_digit()) {
            return true;
        }
    }

    false
}

pub fn slug(text: &str) -> Option<String> {
    let mut parts: Vec<String> = Vec::new();
    let mut current = String::new();

    for ch in text.chars() {
        if ch.is_ascii_alphanumeric() {
            current.push(ch);
        } else if !current.is_empty() {
            parts.push(std::mem::take(&mut current));
        }
        if parts.len() >= 4 {
            break;
        }
    }

    if !current.is_empty() && parts.len() < 4 {
        parts.push(current);
    }

    if parts.is_empty() {
        return None;
    }

    let mut out = String::new();
    for (index, part) in parts.iter().enumerate() {
        let lower = part.to_ascii_lowercase();
        if index == 0 {
            out.push_str(&lower);
        } else {
            let mut chars = lower.chars();
            if let Some(first) = chars.next() {
                out.extend(first.to_uppercase());
                out.push_str(chars.as_str());
            }
        }
        if out.len() > 28 {
            break;
        }
    }

    if out.is_empty() {
        return None;
    }

    if out.chars().next().is_some_and(|ch| ch.is_ascii_digit()) {
        out.insert(0, '_');
    }

    if !is_identifier_name(&out) {
        return None;
    }

    Some(out)
}

#[derive(Debug, Clone, Default)]
pub struct Evidence {
    pub init_hint: Option<String>,
    pub properties: Vec<String>,
    pub distinctive_string: Option<String>,
    pub is_function: bool,
    pub is_parameter: bool,
}

impl Evidence {
    pub fn suggest(&self) -> Option<String> {
        if let Some(hint) = &self.init_hint {
            if let Some(name) = slug(hint) {
                return Some(name);
            }
        }

        if self.is_function {
            if let Some(text) = &self.distinctive_string {
                if let Some(name) = slug(text) {
                    return Some(name);
                }
            }
        }

        let mut counts: BTreeMap<&str, usize> = BTreeMap::new();
        for property in &self.properties {
            if UNINFORMATIVE_PROPERTIES.contains(&property.as_str()) {
                continue;
            }
            *counts.entry(property.as_str()).or_insert(0) += 1;
        }

        let best = counts
            .into_iter()
            .max_by_key(|(name, count)| (*count, name.len()))
            .map(|(name, _)| name.to_string())?;

        slug(&best)
    }
}

#[derive(Default)]
pub struct EvidenceIndex {
    pub entries: HashMap<SymbolId, Evidence>,
}

impl EvidenceIndex {
    pub fn build<'a>(program: &Program<'a>, scoping: &Scoping) -> Self {
        let mut collector = Collector {
            scoping,
            entries: HashMap::new(),
            function_depth: 0,
            current_function: Vec::new(),
        };
        collector.visit_program(program);
        Self { entries: collector.entries }
    }
}

struct Collector<'s> {
    scoping: &'s Scoping,
    entries: HashMap<SymbolId, Evidence>,
    function_depth: usize,
    current_function: Vec<SymbolId>,
}

impl<'s> Collector<'s> {
    fn entry(&mut self, symbol_id: SymbolId) -> &mut Evidence {
        self.entries.entry(symbol_id).or_default()
    }

    fn symbol_of_reference(&self, identifier: &IdentifierReference<'_>) -> Option<SymbolId> {
        let reference_id = identifier.reference_id.get()?;
        self.scoping.get_reference(reference_id).symbol_id()
    }
}

impl<'a, 's> Visit<'a> for Collector<'s> {
    fn visit_variable_declarator(&mut self, declarator: &VariableDeclarator<'a>) {
        if let BindingPattern::BindingIdentifier(identifier) = &declarator.id {
            if let Some(symbol_id) = identifier.symbol_id.get() {
                let hint = declarator.init.as_ref().and_then(initializer_hint);
                let is_function = matches!(
                    declarator.init,
                    Some(Expression::FunctionExpression(_))
                        | Some(Expression::ArrowFunctionExpression(_))
                );

                let entry = self.entry(symbol_id);
                if entry.init_hint.is_none() {
                    entry.init_hint = hint;
                }
                entry.is_function |= is_function;

                if is_function {
                    self.current_function.push(symbol_id);
                    walk::walk_variable_declarator(self, declarator);
                    self.current_function.pop();
                    return;
                }
            }
        }

        if let BindingPattern::ObjectPattern(pattern) = &declarator.id {
            for property in &pattern.properties {
                let key = match &property.key {
                    PropertyKey::StaticIdentifier(identifier) => {
                        Some(identifier.name.as_str().to_string())
                    }
                    PropertyKey::StringLiteral(literal) => {
                        Some(literal.value.as_str().to_string())
                    }
                    _ => None,
                };

                if let (Some(key), BindingPattern::BindingIdentifier(identifier)) =
                    (key, &property.value)
                {
                    if let Some(symbol_id) = identifier.symbol_id.get() {
                        let entry = self.entry(symbol_id);
                        if entry.init_hint.is_none() {
                            entry.init_hint = Some(key);
                        }
                    }
                }
            }
        }

        walk::walk_variable_declarator(self, declarator);
    }

    fn visit_function(&mut self, function: &Function<'a>, flags: oxc_semantic::ScopeFlags) {
        let mut pushed = false;

        if let Some(identifier) = &function.id {
            if let Some(symbol_id) = identifier.symbol_id.get() {
                self.entry(symbol_id).is_function = true;
                self.current_function.push(symbol_id);
                pushed = true;
            }
        }

        for item in &function.params.items {
            if let BindingPattern::BindingIdentifier(identifier) = &item.pattern {
                if let Some(symbol_id) = identifier.symbol_id.get() {
                    self.entry(symbol_id).is_parameter = true;
                }
            }
        }

        self.function_depth += 1;
        walk::walk_function(self, function, flags);
        self.function_depth -= 1;

        if pushed {
            self.current_function.pop();
        }
    }

    fn visit_static_member_expression(&mut self, member: &StaticMemberExpression<'a>) {
        if let Expression::Identifier(identifier) = &member.object {
            if let Some(symbol_id) = self.symbol_of_reference(identifier) {
                let property = member.property.name.as_str().to_string();
                self.entry(symbol_id).properties.push(property);
            }
        }
        walk::walk_static_member_expression(self, member);
    }

    fn visit_string_literal(&mut self, literal: &StringLiteral<'a>) {
        let text = literal.value.as_str();

        if text.len() < 6 || text.len() > 48 {
            return;
        }

        if !text.chars().any(|ch| ch.is_ascii_alphabetic()) {
            return;
        }

        if let Some(symbol_id) = self.current_function.last().copied() {
            let entry = self.entry(symbol_id);
            if entry.distinctive_string.is_none() {
                entry.distinctive_string = Some(text.to_string());
            }
        }
    }
}

fn initializer_hint(expression: &Expression<'_>) -> Option<String> {
    match expression {
        Expression::CallExpression(call) => match &call.callee {
            Expression::StaticMemberExpression(member) => {
                let name = member.property.name.as_str();
                if GENERIC_CALLS.contains(&name) {
                    None
                } else {
                    Some(name.to_string())
                }
            }
            Expression::Identifier(identifier) => {
                let name = identifier.name.as_str();
                if is_junk_name(name) {
                    None
                } else {
                    Some(name.to_string())
                }
            }
            _ => None,
        },
        Expression::NewExpression(call) => match &call.callee {
            Expression::Identifier(identifier) => Some(identifier.name.as_str().to_string()),
            Expression::StaticMemberExpression(member) => {
                Some(member.property.name.as_str().to_string())
            }
            _ => None,
        },
        Expression::StaticMemberExpression(member) => {
            let name = member.property.name.as_str();
            if UNINFORMATIVE_PROPERTIES.contains(&name) {
                None
            } else {
                Some(name.to_string())
            }
        }
        _ => None,
    }
}

pub fn rename_identifiers<'a>(program: &mut Program<'a>, ctx: &mut PassContext<'a>) -> usize {
    if !ctx.config.rename.enabled {
        return 0;
    }

    let Some(scoping) = ctx.scoping() else {
        return 0;
    };

    let evidence = if ctx.config.rename.infer {
        EvidenceIndex::build(program, scoping)
    } else {
        EvidenceIndex::default()
    };

    let mut taken: HashSet<String> = RESERVED.iter().map(|name| (*name).to_string()).collect();
    for name in ctx.config.rename.reserved.iter() {
        taken.insert(name.clone());
    }
    for symbol_id in scoping.symbol_ids() {
        let name = scoping.symbol_name(symbol_id);
        if !is_junk_name(name) {
            taken.insert(name.to_string());
        }
    }

    let mut mapping: HashMap<SymbolId, String> = HashMap::new();
    let mut counters: HashMap<String, usize> = HashMap::new();

    let mut symbols: Vec<SymbolId> = scoping.symbol_ids().collect();
    symbols.sort_by_key(|symbol_id| symbol_id.index());

    for symbol_id in symbols {
        let original = scoping.symbol_name(symbol_id).to_string();

        if let Some(forced) = ctx.config.rename.forced.get(&original) {
            mapping.insert(symbol_id, forced.clone());
            taken.insert(forced.clone());
            continue;
        }

        if !is_junk_name(&original) {
            continue;
        }

        let facts = evidence.entries.get(&symbol_id);
        let is_function = facts.map(|entry| entry.is_function).unwrap_or(false)
            || scoping
                .symbol_flags(symbol_id)
                .contains(oxc_semantic::SymbolFlags::Function);

        let base = facts
            .and_then(|entry| entry.suggest())
            .filter(|name| ctx.config.rename.infer && !name.is_empty())
            .unwrap_or_else(|| if is_function { "fn".to_string() } else { "v".to_string() });

        let candidate = unique_name(&base, &mut counters, &taken);
        taken.insert(candidate.clone());
        mapping.insert(symbol_id, candidate);
    }

    if mapping.is_empty() {
        return 0;
    }

    let scoping_ref = ctx.scoping.take().expect("scoping checked above");
    let mut pass = ApplyRenames {
        builder_alloc: ctx.builder.allocator(),
        scoping: &scoping_ref,
        mapping,
        changed: 0,
    };
    pass.visit_program(program);
    let changed = pass.changed;
    ctx.scoping = Some(scoping_ref);

    changed
}

fn unique_name(
    base: &str,
    counters: &mut HashMap<String, usize>,
    taken: &HashSet<String>,
) -> String {
    if !taken.contains(base) && !counters.contains_key(base) {
        counters.insert(base.to_string(), 1);
        return base.to_string();
    }

    let counter = counters.entry(base.to_string()).or_insert(1);
    loop {
        *counter += 1;
        let candidate = format!("{base}{counter}");
        if !taken.contains(&candidate) {
            return candidate;
        }
    }
}

struct ApplyRenames<'a, 's> {
    builder_alloc: &'a oxc_allocator::Allocator,
    scoping: &'s Scoping,
    mapping: HashMap<SymbolId, String>,
    changed: usize,
}

impl<'a, 's> VisitMut<'a> for ApplyRenames<'a, 's> {
    fn visit_binding_identifier(&mut self, it: &mut BindingIdentifier<'a>) {
        walk_mut::walk_binding_identifier(self, it);

        let Some(symbol_id) = it.symbol_id.get() else {
            return;
        };

        let Some(name) = self.mapping.get(&symbol_id) else {
            return;
        };

        let arena: &'a str = self.builder_alloc.alloc_str(name);
        it.name = arena.into();
        self.changed += 1;
    }

    fn visit_identifier_reference(&mut self, it: &mut IdentifierReference<'a>) {
        walk_mut::walk_identifier_reference(self, it);

        let Some(reference_id) = it.reference_id.get() else {
            return;
        };

        let Some(symbol_id) = self.scoping.get_reference(reference_id).symbol_id() else {
            return;
        };

        let Some(name) = self.mapping.get(&symbol_id) else {
            return;
        };

        let arena: &'a str = self.builder_alloc.alloc_str(name);
        it.name = arena.into();
        self.changed += 1;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recognises_junk_names() {
        assert!(is_junk_name("_0x4a2f"));
        assert!(is_junk_name("0x1b"));
        assert!(is_junk_name("a"));
        assert!(is_junk_name("t7"));
        assert!(is_junk_name("_12"));
        assert!(!is_junk_name("deriveKey"));
        assert!(!is_junk_name("canvasHash"));
    }

    #[test]
    fn slugs_readable_names() {
        assert_eq!(slug("createElement").as_deref(), Some("createelement"));
        assert_eq!(slug("Worker is already running").as_deref(), Some("workerIsAlreadyRunning"));
        assert_eq!(slug("!!!").as_deref(), None);
        assert_eq!(slug("2fast").as_deref(), Some("_2fast"));
    }
}
