use std::collections::{HashMap, HashSet};

use oxc_allocator::Vec as ArenaVec;
use oxc_ast::ast::*;
use oxc_ast_visit::{Visit, VisitMut, walk, walk_mut};
use oxc_semantic::Scoping;
use oxc_span::{GetSpan, Span};

use crate::eval::{Const, eval, is_pure, to_expression};
use crate::pipeline::PassContext;

#[derive(Debug, Clone, Default)]
pub struct NameFacts {
    pub declarations: usize,
    pub mutated: bool,
    pub references: usize,
}

pub fn name_facts(scoping: &Scoping) -> HashMap<String, NameFacts> {
    let mut out: HashMap<String, NameFacts> = HashMap::new();

    for symbol_id in scoping.symbol_ids() {
        let name = scoping.symbol_name(symbol_id).to_string();
        let entry = out.entry(name).or_default();
        entry.declarations += 1;
        entry.mutated |= scoping.symbol_is_mutated(symbol_id);
        entry.references += scoping.get_resolved_references(symbol_id).count();
    }

    out
}

pub fn unambiguous(facts: &HashMap<String, NameFacts>, name: &str) -> bool {
    facts
        .get(name)
        .map(|entry| entry.declarations == 1)
        .unwrap_or(false)
}

pub fn declared_names(scoping: &Scoping) -> HashSet<String> {
    scoping
        .symbol_ids()
        .map(|symbol_id| scoping.symbol_name(symbol_id).to_string())
        .collect()
}

pub fn inline_constant_bindings<'a>(
    program: &mut Program<'a>,
    ctx: &mut PassContext<'a>,
) -> usize {
    let Some(scoping) = ctx.scoping() else {
        return 0;
    };

    let facts = name_facts(scoping);

    let mut collector = ConstantCollector { values: HashMap::new() };
    collector.visit_program(program);

    let values: HashMap<String, Const> = collector
        .values
        .into_iter()
        .filter(|(name, _)| {
            unambiguous(&facts, name) && !facts.get(name).map(|f| f.mutated).unwrap_or(true)
        })
        .collect();

    if values.is_empty() {
        return 0;
    }

    let names: HashSet<String> = values.keys().cloned().collect();

    let mut replacer = ReplaceReferences { ctx, values, changed: 0 };
    replacer.visit_program(program);
    let changed = replacer.changed;

    if changed > 0 {
        let mut remover = RemoveNamedDeclarations { names, changed: 0 };
        remover.visit_program(program);
    }

    changed
}

#[derive(Default)]
struct ConstantCollector {
    values: HashMap<String, Const>,
}

impl<'a> Visit<'a> for ConstantCollector {
    fn visit_variable_declarator(&mut self, declarator: &VariableDeclarator<'a>) {
        walk::walk_variable_declarator(self, declarator);

        let BindingPattern::BindingIdentifier(identifier) = &declarator.id else {
            return;
        };

        let Some(init) = &declarator.init else { return };

        let literal = matches!(
            init,
            Expression::NumericLiteral(_)
                | Expression::StringLiteral(_)
                | Expression::BooleanLiteral(_)
                | Expression::NullLiteral(_)
        );

        if !literal {
            return;
        }

        if let Some(value) = eval(init) {
            if let Const::Text(text) = &value {
                if text.len() > 512 {
                    return;
                }
            }
            self.values
                .insert(identifier.name.as_str().to_string(), value);
        }
    }
}

struct ReplaceReferences<'a, 'c> {
    ctx: &'c mut PassContext<'a>,
    values: HashMap<String, Const>,
    changed: usize,
}

impl<'a, 'c> VisitMut<'a> for ReplaceReferences<'a, 'c> {
    fn visit_expression(&mut self, it: &mut Expression<'a>) {
        walk_mut::walk_expression(self, it);

        let Expression::Identifier(identifier) = it else {
            return;
        };

        let Some(value) = self.values.get(identifier.name.as_str()).cloned() else {
            return;
        };

        let span = it.span();
        if let Some(replacement) = to_expression(&value, span, &self.ctx.builder) {
            *it = replacement;
            self.changed += 1;
        }
    }
}

struct RemoveNamedDeclarations {
    names: HashSet<String>,
    changed: usize,
}

impl<'a> VisitMut<'a> for RemoveNamedDeclarations {
    fn visit_statements(&mut self, it: &mut ArenaVec<'a, Statement<'a>>) {
        walk_mut::walk_statements(self, it);

        let names = &self.names;
        let before = it.len();

        it.retain(|statement| match statement {
            Statement::VariableDeclaration(declaration) => {
                !declaration.declarations.iter().all(|declarator| {
                    match &declarator.id {
                        BindingPattern::BindingIdentifier(identifier) => {
                            names.contains(identifier.name.as_str())
                        }
                        _ => false,
                    }
                })
            }
            _ => true,
        });

        self.changed += before - it.len();
    }
}

pub fn inline_global_aliases<'a>(program: &mut Program<'a>, ctx: &mut PassContext<'a>) -> usize {
    if !ctx.config.inline_global_aliases {
        return 0;
    }

    let Some(scoping) = ctx.scoping() else {
        return 0;
    };

    let facts = name_facts(scoping);
    let declared = declared_names(scoping);

    let mut collector = AliasCollector { aliases: HashMap::new() };
    collector.visit_program(program);

    let aliases: HashMap<String, String> = collector
        .aliases
        .into_iter()
        .filter(|(name, target)| {
            unambiguous(&facts, name)
                && !facts.get(name).map(|entry| entry.mutated).unwrap_or(true)
                && !declared.contains(target)
                && name != target
        })
        .collect();

    if aliases.is_empty() {
        return 0;
    }

    let names: HashSet<String> = aliases.keys().cloned().collect();

    let mut replacer = ReplaceAliases { ctx, aliases, changed: 0 };
    replacer.visit_program(program);
    let changed = replacer.changed;

    if changed > 0 {
        let mut remover = RemoveNamedDeclarations { names, changed: 0 };
        remover.visit_program(program);
    }

    changed
}

#[derive(Default)]
struct AliasCollector {
    aliases: HashMap<String, String>,
}

impl<'a> Visit<'a> for AliasCollector {
    fn visit_variable_declarator(&mut self, declarator: &VariableDeclarator<'a>) {
        walk::walk_variable_declarator(self, declarator);

        let BindingPattern::BindingIdentifier(identifier) = &declarator.id else {
            return;
        };

        let Some(Expression::Identifier(target)) = &declarator.init else {
            return;
        };

        self.aliases.insert(
            identifier.name.as_str().to_string(),
            target.name.as_str().to_string(),
        );
    }
}

struct ReplaceAliases<'a, 'c> {
    ctx: &'c mut PassContext<'a>,
    aliases: HashMap<String, String>,
    changed: usize,
}

impl<'a, 'c> VisitMut<'a> for ReplaceAliases<'a, 'c> {
    fn visit_expression(&mut self, it: &mut Expression<'a>) {
        walk_mut::walk_expression(self, it);

        let Expression::Identifier(identifier) = it else {
            return;
        };

        let Some(target) = self.aliases.get(identifier.name.as_str()).cloned() else {
            return;
        };

        let span = it.span();
        let name = self.ctx.alloc(&target);
        *it = Expression::new_identifier(span, name, &self.ctx.builder);
        self.changed += 1;
    }
}

pub fn remove_unused_bindings<'a>(program: &mut Program<'a>, ctx: &mut PassContext<'a>) -> usize {
    if !ctx.config.remove_unused {
        return 0;
    }

    let Some(scoping) = ctx.scoping() else {
        return 0;
    };

    let facts = name_facts(scoping);

    let unused: HashSet<String> = facts
        .iter()
        .filter(|(_, entry)| entry.declarations == 1 && entry.references == 0)
        .map(|(name, _)| name.clone())
        .collect();

    if unused.is_empty() {
        return 0;
    }

    let mut pass = RemoveUnused { names: unused, changed: 0 };
    pass.visit_program(program);
    let _ = ctx;
    pass.changed
}

struct RemoveUnused {
    names: HashSet<String>,
    changed: usize,
}

impl<'a> VisitMut<'a> for RemoveUnused {
    fn visit_statements(&mut self, it: &mut ArenaVec<'a, Statement<'a>>) {
        walk_mut::walk_statements(self, it);

        let names = &self.names;
        let before = it.len();

        it.retain(|statement| match statement {
            Statement::VariableDeclaration(declaration) => {
                !declaration.declarations.iter().all(|declarator| {
                    let BindingPattern::BindingIdentifier(identifier) = &declarator.id
                    else {
                        return false;
                    };
                    if !names.contains(identifier.name.as_str()) {
                        return false;
                    }
                    declarator
                        .init
                        .as_ref()
                        .map(is_pure)
                        .unwrap_or(true)
                })
            }
            Statement::FunctionDeclaration(function) => function
                .id
                .as_ref()
                .map(|identifier| !names.contains(identifier.name.as_str()))
                .unwrap_or(true),
            _ => true,
        });

        self.changed += before - it.len();
    }
}

pub fn empty_span() -> Span {
    Span::default()
}
