use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet, VecDeque};

use oxc_allocator::Allocator;
use oxc_ast::ast::*;
use oxc_ast_visit::{Visit, walk};
use oxc_parser::{ParseOptions, Parser};
use oxc_semantic::{Scoping, SemanticBuilder, SymbolId};
use serde::{Deserialize, Serialize};

use wre_core::error::{Error, Result};

use crate::pipeline::SourceKind;

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct FunctionSurface {
    pub name: String,
    pub member_paths: BTreeSet<String>,
    pub globals: BTreeSet<String>,
    pub strings: Vec<String>,
    pub numbers: Vec<f64>,
    pub calls: BTreeSet<String>,
    pub throws: usize,
    pub returns: usize,
    pub branches: usize,
    pub loops: usize,
    pub statements: usize,
}

impl FunctionSurface {
    pub fn rarest(&self, frequency: &BTreeMap<String, usize>) -> Option<String> {
        self.member_paths
            .iter()
            .chain(self.globals.iter())
            .min_by_key(|path| (frequency.get(*path).copied().unwrap_or(0), path.len()))
            .cloned()
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SurfaceIndex {
    pub functions: BTreeMap<String, FunctionSurface>,
    pub frequency: BTreeMap<String, usize>,
    pub call_graph: BTreeMap<String, BTreeSet<String>>,
}

impl SurfaceIndex {
    pub fn build(source: &str, kind: SourceKind) -> Result<Self> {
        let allocator = Allocator::default();
        let parsed = Parser::new(&allocator, source, kind.to_source_type())
            .with_options(ParseOptions {
                preserve_parens: false,
                ..ParseOptions::default()
            })
            .parse();

        if parsed.panicked {
            return Err(Error::msg("surface index could not parse the source"));
        }

        let program = parsed.program;
        let scoping: Scoping = SemanticBuilder::new().build(&program).semantic.into_scoping();

        let mut collector = SurfaceCollector {
            scoping: &scoping,
            stack: Vec::new(),
            functions: BTreeMap::new(),
            call_graph: BTreeMap::new(),
            declared: scoping
                .symbol_ids()
                .map(|symbol_id| scoping.symbol_name(symbol_id).to_string())
                .collect(),
        };

        collector.visit_program(&program);

        let mut frequency: BTreeMap<String, usize> = BTreeMap::new();
        for surface in collector.functions.values() {
            for path in surface.member_paths.iter().chain(surface.globals.iter()) {
                *frequency.entry(path.clone()).or_insert(0) += 1;
            }
        }

        Ok(Self {
            functions: collector.functions,
            frequency,
            call_graph: collector.call_graph,
        })
    }

    pub fn reachable_from(&self, root: &str) -> BTreeSet<String> {
        let mut seen = BTreeSet::new();
        let mut queue = VecDeque::new();
        queue.push_back(root.to_string());

        while let Some(current) = queue.pop_front() {
            if !seen.insert(current.clone()) {
                continue;
            }
            if let Some(callees) = self.call_graph.get(&current) {
                for callee in callees {
                    queue.push_back(callee.clone());
                }
            }
        }

        seen.remove(root);
        seen
    }

    pub fn exclusive_owners(&self, roots: &[String]) -> BTreeMap<String, String> {
        let mut owners: HashMap<String, HashSet<String>> = HashMap::new();

        for root in roots {
            let mut seen = HashSet::new();
            let mut queue = VecDeque::new();
            queue.push_back(root.clone());

            while let Some(current) = queue.pop_front() {
                if !seen.insert(current.clone()) {
                    continue;
                }

                if &current != root && roots.contains(&current) {
                    continue;
                }

                if &current != root {
                    owners.entry(current.clone()).or_default().insert(root.clone());
                }

                if let Some(callees) = self.call_graph.get(&current) {
                    for callee in callees {
                        queue.push_back(callee.clone());
                    }
                }
            }
        }

        owners
            .into_iter()
            .filter(|(_, roots)| roots.len() == 1)
            .map(|(helper, roots)| {
                let owner = roots.into_iter().next().expect("exactly one");
                (helper, owner)
            })
            .collect()
    }

    pub fn signature(&self, name: &str) -> Option<String> {
        self.functions
            .get(name)
            .and_then(|surface| surface.rarest(&self.frequency))
    }
}

struct SurfaceCollector<'s> {
    scoping: &'s Scoping,
    stack: Vec<String>,
    functions: BTreeMap<String, FunctionSurface>,
    call_graph: BTreeMap<String, BTreeSet<String>>,
    declared: HashSet<String>,
}

impl<'s> SurfaceCollector<'s> {
    fn current(&mut self) -> Option<&mut FunctionSurface> {
        let name = self.stack.last()?.clone();
        self.functions.get_mut(&name)
    }

    fn enter(&mut self, name: String) {
        self.functions
            .entry(name.clone())
            .or_insert_with(|| FunctionSurface { name: name.clone(), ..FunctionSurface::default() });
        self.stack.push(name);
    }

    fn leave(&mut self) {
        self.stack.pop();
    }

    fn record_call(&mut self, callee: &str) {
        if let Some(caller) = self.stack.last().cloned() {
            self.call_graph
                .entry(caller)
                .or_default()
                .insert(callee.to_string());
        }
    }

    fn symbol_of(&self, identifier: &IdentifierReference<'_>) -> Option<SymbolId> {
        let reference_id = identifier.reference_id.get()?;
        self.scoping.get_reference(reference_id).symbol_id()
    }
}

impl<'a, 's> Visit<'a> for SurfaceCollector<'s> {
    fn visit_function(&mut self, function: &Function<'a>, flags: oxc_semantic::ScopeFlags) {
        let name = function
            .id
            .as_ref()
            .map(|identifier| identifier.name.as_str().to_string())
            .unwrap_or_else(|| format!("anonymous@{}", function.span.start));

        self.enter(name);
        walk::walk_function(self, function, flags);
        self.leave();
    }

    fn visit_variable_declarator(&mut self, declarator: &VariableDeclarator<'a>) {
        let named_function = matches!(
            declarator.init,
            Some(Expression::FunctionExpression(_)) | Some(Expression::ArrowFunctionExpression(_))
        );

        if named_function {
            if let BindingPattern::BindingIdentifier(identifier) = &declarator.id {
                self.enter(identifier.name.as_str().to_string());
                walk::walk_variable_declarator(self, declarator);
                self.leave();
                return;
            }
        }

        walk::walk_variable_declarator(self, declarator);
    }

    fn visit_static_member_expression(&mut self, member: &StaticMemberExpression<'a>) {
        if let Some(path) = member_path(&member.object, member.property.name.as_str()) {
            if let Some(surface) = self.current() {
                surface.member_paths.insert(path);
            }
        }
        walk::walk_static_member_expression(self, member);
    }

    fn visit_identifier_reference(&mut self, identifier: &IdentifierReference<'a>) {
        let name = identifier.name.as_str().to_string();
        let resolved = self.symbol_of(identifier).is_some();

        if !resolved && !self.declared.contains(&name) {
            if let Some(surface) = self.current() {
                surface.globals.insert(name);
            }
        }

        walk::walk_identifier_reference(self, identifier);
    }

    fn visit_call_expression(&mut self, call: &CallExpression<'a>) {
        match &call.callee {
            Expression::Identifier(identifier) => {
                let name = identifier.name.as_str().to_string();
                self.record_call(&name);
                if let Some(surface) = self.current() {
                    surface.calls.insert(name);
                }
            }
            Expression::StaticMemberExpression(member) => {
                let name = member.property.name.as_str().to_string();
                if let Some(surface) = self.current() {
                    surface.calls.insert(name);
                }
            }
            _ => {}
        }

        walk::walk_call_expression(self, call);
    }

    fn visit_string_literal(&mut self, literal: &StringLiteral<'a>) {
        if let Some(surface) = self.current() {
            if surface.strings.len() < 64 {
                surface.strings.push(literal.value.as_str().to_string());
            }
        }
    }

    fn visit_numeric_literal(&mut self, literal: &NumericLiteral<'a>) {
        if let Some(surface) = self.current() {
            if surface.numbers.len() < 64 {
                surface.numbers.push(literal.value);
            }
        }
    }

    fn visit_throw_statement(&mut self, statement: &ThrowStatement<'a>) {
        if let Some(surface) = self.current() {
            surface.throws += 1;
        }
        walk::walk_throw_statement(self, statement);
    }

    fn visit_return_statement(&mut self, statement: &ReturnStatement<'a>) {
        if let Some(surface) = self.current() {
            surface.returns += 1;
        }
        walk::walk_return_statement(self, statement);
    }

    fn visit_if_statement(&mut self, statement: &IfStatement<'a>) {
        if let Some(surface) = self.current() {
            surface.branches += 1;
        }
        walk::walk_if_statement(self, statement);
    }

    fn visit_for_statement(&mut self, statement: &ForStatement<'a>) {
        if let Some(surface) = self.current() {
            surface.loops += 1;
        }
        walk::walk_for_statement(self, statement);
    }

    fn visit_while_statement(&mut self, statement: &WhileStatement<'a>) {
        if let Some(surface) = self.current() {
            surface.loops += 1;
        }
        walk::walk_while_statement(self, statement);
    }

    fn visit_statement(&mut self, statement: &Statement<'a>) {
        if let Some(surface) = self.current() {
            surface.statements += 1;
        }
        walk::walk_statement(self, statement);
    }
}

fn member_path(object: &Expression<'_>, property: &str) -> Option<String> {
    match object {
        Expression::Identifier(identifier) => {
            Some(format!("{}.{}", identifier.name.as_str(), property))
        }
        Expression::StaticMemberExpression(member) => {
            let inner = member_path(&member.object, member.property.name.as_str())?;
            Some(format!("{inner}.{property}"))
        }
        Expression::ThisExpression(_) => Some(format!("this.{property}")),
        _ => None,
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SignatureRule {
    pub role: String,
    pub pattern: String,
    #[serde(default)]
    pub params: Option<usize>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct RoleMap {
    pub roles: BTreeMap<String, String>,
    pub unmatched: Vec<String>,
}

pub fn detect_roles(source: &str, kind: SourceKind, rules: &[SignatureRule]) -> Result<RoleMap> {
    let allocator = Allocator::default();
    let parsed = Parser::new(&allocator, source, kind.to_source_type())
        .with_options(ParseOptions {
            preserve_parens: false,
            ..ParseOptions::default()
        })
        .parse();

    if parsed.panicked {
        return Err(Error::msg("role detection could not parse the source"));
    }

    let mut compiled = Vec::with_capacity(rules.len());
    for rule in rules {
        let regex = regex::Regex::new(&rule.pattern)
            .map_err(|error| Error::msg(format!("bad signature for {}: {error}", rule.role)))?;
        compiled.push((rule, regex));
    }

    let mut collector = TopLevelFunctions { entries: Vec::new() };
    collector.visit_program(&parsed.program);

    let mut roles = BTreeMap::new();
    let mut claimed: HashSet<String> = HashSet::new();

    for (name, params, text) in &collector.entries {
        for (rule, regex) in &compiled {
            if roles.contains_key(&rule.role) || claimed.contains(name) {
                continue;
            }
            if let Some(expected) = rule.params {
                if *params != expected {
                    continue;
                }
            }
            if regex.is_match(text) {
                roles.insert(rule.role.clone(), name.clone());
                claimed.insert(name.clone());
            }
        }
    }

    let unmatched = rules
        .iter()
        .filter(|rule| !roles.contains_key(&rule.role))
        .map(|rule| rule.role.clone())
        .collect();

    Ok(RoleMap { roles, unmatched })
}

struct TopLevelFunctions {
    entries: Vec<(String, usize, String)>,
}

impl<'a> Visit<'a> for TopLevelFunctions {
    fn visit_program(&mut self, program: &Program<'a>) {
        for statement in &program.body {
            match statement {
                Statement::FunctionDeclaration(function) => {
                    if let Some(identifier) = &function.id {
                        self.entries.push((
                            identifier.name.as_str().to_string(),
                            function.params.items.len(),
                            print_span(function.span, program),
                        ));
                    }
                }
                Statement::VariableDeclaration(declaration) => {
                    for declarator in &declaration.declarations {
                        let BindingPattern::BindingIdentifier(identifier) = &declarator.id
                        else {
                            continue;
                        };

                        let (params, span) = match &declarator.init {
                            Some(Expression::FunctionExpression(function)) => {
                                (function.params.items.len(), function.span)
                            }
                            Some(Expression::ArrowFunctionExpression(arrow)) => {
                                (arrow.params.items.len(), arrow.span)
                            }
                            _ => continue,
                        };

                        self.entries.push((
                            identifier.name.as_str().to_string(),
                            params,
                            print_span(span, program),
                        ));
                    }
                }
                _ => {}
            }
        }
    }
}

fn print_span(span: oxc_span::Span, program: &Program<'_>) -> String {
    let source = program.source_text;
    let start = span.start as usize;
    let end = (span.end as usize).min(source.len());

    if start >= end || !source.is_char_boundary(start) || !source.is_char_boundary(end) {
        return String::new();
    }

    source[start..end].to_string()
}
