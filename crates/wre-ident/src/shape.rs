use std::collections::{BTreeMap, BTreeSet};
use std::hash::Hasher;

use oxc_allocator::Allocator;
use oxc_ast::ast::*;
use oxc_ast_visit::{Visit, walk};
use oxc_parser::{ParseOptions, Parser};
use serde::{Deserialize, Serialize};
use twox_hash::XxHash64;

use wre_core::error::{Error, Result};
use wre_js::pipeline::SourceKind;

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Shape {
    pub tokens: Vec<String>,
    pub skeleton: Vec<String>,
}

impl Shape {
    pub fn text(&self) -> String {
        self.tokens.join(" ")
    }

    pub fn skeleton_text(&self) -> String {
        self.skeleton.join(" ")
    }

    pub fn text_hash(&self) -> u64 {
        hash_all(&self.tokens)
    }

    pub fn skeleton_hash(&self) -> u64 {
        hash_all(&self.skeleton)
    }

    pub fn grams(&self, width: usize) -> BTreeMap<u64, usize> {
        let width = width.max(1);
        let mut out: BTreeMap<u64, usize> = BTreeMap::new();

        if self.skeleton.is_empty() {
            return out;
        }

        if self.skeleton.len() < width {
            out.insert(hash_all(&self.skeleton), 1);
            return out;
        }

        for window in self.skeleton.windows(width) {
            *out.entry(hash_all(window)).or_insert(0) += 1;
        }

        out
    }

    pub fn similarity(&self, other: &Shape, width: usize) -> f64 {
        overlap(&self.grams(width), &other.grams(width))
    }

    pub fn len(&self) -> usize {
        self.skeleton.len()
    }

    pub fn is_empty(&self) -> bool {
        self.skeleton.is_empty()
    }
}

pub fn overlap(left: &BTreeMap<u64, usize>, right: &BTreeMap<u64, usize>) -> f64 {
    if left.is_empty() && right.is_empty() {
        return 1.0;
    }

    let mut keys: BTreeSet<u64> = left.keys().copied().collect();
    keys.extend(right.keys().copied());

    let mut shared = 0usize;
    let mut total = 0usize;

    for key in keys {
        let mine = left.get(&key).copied().unwrap_or(0);
        let theirs = right.get(&key).copied().unwrap_or(0);
        shared += mine.min(theirs);
        total += mine.max(theirs);
    }

    if total == 0 { 0.0 } else { shared as f64 / total as f64 }
}

fn hash_all(parts: &[String]) -> u64 {
    let mut hasher = XxHash64::with_seed(0);
    for part in parts {
        hasher.write(part.as_bytes());
        hasher.write_u8(0);
    }
    hasher.finish()
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Facts {
    pub numbers: Vec<f64>,
    pub strings: Vec<String>,
    pub properties: BTreeSet<String>,
    pub calls: BTreeSet<String>,
    pub object_keys: Vec<Vec<String>>,
    pub statements: usize,
    pub loops: usize,
    pub branches: usize,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct FunctionShape {
    pub name: String,
    pub params: usize,
    pub start: u32,
    pub end: u32,
    pub shape: Shape,
    pub facts: Facts,
}

impl FunctionShape {
    pub fn text(&self) -> String {
        self.shape.text()
    }

    pub fn has_number(&self, wanted: f64) -> bool {
        self.facts
            .numbers
            .iter()
            .any(|value| (value - wanted).abs() < f64::EPSILON)
    }

    pub fn has_object_with(&self, keys: &[String]) -> bool {
        self.facts
            .object_keys
            .iter()
            .any(|found| keys.iter().all(|key| found.contains(key)))
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ShapeIndex {
    pub functions: Vec<FunctionShape>,
}

impl ShapeIndex {
    pub fn build(source: &str, kind: SourceKind) -> Result<Self> {
        let allocator = Allocator::default();
        let parsed = Parser::new(&allocator, source, kind.to_source_type())
            .with_options(ParseOptions {
                preserve_parens: false,
                ..ParseOptions::default()
            })
            .parse();

        if parsed.panicked {
            return Err(Error::msg("shape index could not parse the source"));
        }

        let mut collector = TopLevel { functions: Vec::new() };
        collector.visit_program(&parsed.program);

        Ok(Self { functions: collector.functions })
    }

    pub fn get(&self, name: &str) -> Option<&FunctionShape> {
        self.functions.iter().find(|entry| entry.name == name)
    }

    pub fn names(&self) -> Vec<String> {
        self.functions.iter().map(|entry| entry.name.clone()).collect()
    }
}

struct TopLevel {
    functions: Vec<FunctionShape>,
}

impl TopLevel {
    fn record(
        &mut self,
        name: String,
        params: usize,
        span: oxc_span::Span,
        walker: ShapeWalker,
    ) {
        let (shape, facts) = walker.finish();
        self.functions.push(FunctionShape {
            name,
            params,
            start: span.start,
            end: span.end,
            shape,
            facts,
        });
    }
}

impl<'a> Visit<'a> for TopLevel {
    fn visit_program(&mut self, program: &Program<'a>) {
        for statement in &program.body {
            match statement {
                Statement::FunctionDeclaration(function) => {
                    let Some(identifier) = &function.id else {
                        continue;
                    };

                    let mut walker = ShapeWalker::default();
                    walker.visit_function(function, oxc_semantic::ScopeFlags::Function);

                    self.record(
                        identifier.name.as_str().to_string(),
                        function.params.items.len(),
                        function.span,
                        walker,
                    );
                }
                Statement::VariableDeclaration(declaration) => {
                    for declarator in &declaration.declarations {
                        let BindingPattern::BindingIdentifier(identifier) = &declarator.id else {
                            continue;
                        };

                        let mut walker = ShapeWalker::default();
                        let (params, span) = match &declarator.init {
                            Some(Expression::FunctionExpression(function)) => {
                                walker.visit_function(
                                    function,
                                    oxc_semantic::ScopeFlags::Function,
                                );
                                (function.params.items.len(), function.span)
                            }
                            Some(Expression::ArrowFunctionExpression(arrow)) => {
                                walker.visit_arrow_function_expression(arrow);
                                (arrow.params.items.len(), arrow.span)
                            }
                            _ => continue,
                        };

                        self.record(
                            identifier.name.as_str().to_string(),
                            params,
                            span,
                            walker,
                        );
                    }
                }
                _ => {}
            }
        }
    }
}

#[derive(Default)]
struct ShapeWalker {
    tokens: Vec<String>,
    skeleton: Vec<String>,
    facts: Facts,
}

impl ShapeWalker {
    fn both(&mut self, token: &str) {
        self.tokens.push(token.to_string());
        self.skeleton.push(token.to_string());
    }

    fn detail(&mut self, token: String) {
        self.tokens.push(token);
    }

    fn finish(self) -> (Shape, Facts) {
        (Shape { tokens: self.tokens, skeleton: self.skeleton }, self.facts)
    }
}

impl<'a> Visit<'a> for ShapeWalker {
    fn visit_statement(&mut self, statement: &Statement<'a>) {
        self.facts.statements += 1;
        walk::walk_statement(self, statement);
    }

    fn visit_if_statement(&mut self, statement: &IfStatement<'a>) {
        self.both("if");
        self.facts.branches += 1;
        walk::walk_if_statement(self, statement);
    }

    fn visit_for_statement(&mut self, statement: &ForStatement<'a>) {
        self.both("for");
        self.facts.loops += 1;
        walk::walk_for_statement(self, statement);
    }

    fn visit_for_in_statement(&mut self, statement: &ForInStatement<'a>) {
        self.both("forin");
        self.facts.loops += 1;
        walk::walk_for_in_statement(self, statement);
    }

    fn visit_for_of_statement(&mut self, statement: &ForOfStatement<'a>) {
        self.both("forof");
        self.facts.loops += 1;
        walk::walk_for_of_statement(self, statement);
    }

    fn visit_while_statement(&mut self, statement: &WhileStatement<'a>) {
        self.both("while");
        self.facts.loops += 1;
        walk::walk_while_statement(self, statement);
    }

    fn visit_do_while_statement(&mut self, statement: &DoWhileStatement<'a>) {
        self.both("dowhile");
        self.facts.loops += 1;
        walk::walk_do_while_statement(self, statement);
    }

    fn visit_switch_statement(&mut self, statement: &SwitchStatement<'a>) {
        self.both("switch");
        self.detail(format!("cases:{}", statement.cases.len()));
        walk::walk_switch_statement(self, statement);
    }

    fn visit_try_statement(&mut self, statement: &TryStatement<'a>) {
        self.both("try");
        walk::walk_try_statement(self, statement);
    }

    fn visit_throw_statement(&mut self, statement: &ThrowStatement<'a>) {
        self.both("throw");
        walk::walk_throw_statement(self, statement);
    }

    fn visit_return_statement(&mut self, statement: &ReturnStatement<'a>) {
        self.both("return");
        walk::walk_return_statement(self, statement);
    }

    fn visit_break_statement(&mut self, statement: &BreakStatement<'a>) {
        self.both("break");
        walk::walk_break_statement(self, statement);
    }

    fn visit_continue_statement(&mut self, statement: &ContinueStatement<'a>) {
        self.both("continue");
        walk::walk_continue_statement(self, statement);
    }

    fn visit_variable_declaration(&mut self, declaration: &VariableDeclaration<'a>) {
        self.both("declare");
        walk::walk_variable_declaration(self, declaration);
    }

    fn visit_binary_expression(&mut self, expression: &BinaryExpression<'a>) {
        self.both("binary");
        self.detail(expression.operator.as_str().to_string());
        walk::walk_binary_expression(self, expression);
    }

    fn visit_logical_expression(&mut self, expression: &LogicalExpression<'a>) {
        self.both("logical");
        self.detail(expression.operator.as_str().to_string());
        walk::walk_logical_expression(self, expression);
    }

    fn visit_unary_expression(&mut self, expression: &UnaryExpression<'a>) {
        self.both("unary");
        self.detail(expression.operator.as_str().to_string());
        walk::walk_unary_expression(self, expression);
    }

    fn visit_update_expression(&mut self, expression: &UpdateExpression<'a>) {
        self.both("update");
        self.detail(expression.operator.as_str().to_string());
        walk::walk_update_expression(self, expression);
    }

    fn visit_assignment_expression(&mut self, expression: &AssignmentExpression<'a>) {
        self.both("assign");
        self.detail(expression.operator.as_str().to_string());
        walk::walk_assignment_expression(self, expression);
    }

    fn visit_conditional_expression(&mut self, expression: &ConditionalExpression<'a>) {
        self.both("ternary");
        walk::walk_conditional_expression(self, expression);
    }

    fn visit_sequence_expression(&mut self, expression: &SequenceExpression<'a>) {
        self.both("sequence");
        walk::walk_sequence_expression(self, expression);
    }

    fn visit_call_expression(&mut self, expression: &CallExpression<'a>) {
        self.both("call");
        self.detail(format!("args:{}", expression.arguments.len()));

        if let Expression::Identifier(identifier) = &expression.callee {
            self.facts.calls.insert(identifier.name.as_str().to_string());
        }

        walk::walk_call_expression(self, expression);
    }

    fn visit_new_expression(&mut self, expression: &NewExpression<'a>) {
        self.both("new");
        walk::walk_new_expression(self, expression);
    }

    fn visit_static_member_expression(&mut self, expression: &StaticMemberExpression<'a>) {
        self.both("member");
        self.detail(format!(".{}", expression.property.name.as_str()));
        self.facts
            .properties
            .insert(expression.property.name.as_str().to_string());
        walk::walk_static_member_expression(self, expression);
    }

    fn visit_computed_member_expression(&mut self, expression: &ComputedMemberExpression<'a>) {
        self.both("index");
        walk::walk_computed_member_expression(self, expression);
    }

    fn visit_object_expression(&mut self, expression: &ObjectExpression<'a>) {
        self.both("object");
        self.detail(format!("props:{}", expression.properties.len()));

        let keys: Vec<String> = expression
            .properties
            .iter()
            .filter_map(|property| match property {
                ObjectPropertyKind::ObjectProperty(entry) => {
                    entry.key.static_name().map(|name| name.to_string())
                }
                ObjectPropertyKind::SpreadProperty(_) => None,
            })
            .collect();

        if !keys.is_empty() {
            self.facts.object_keys.push(keys);
        }

        walk::walk_object_expression(self, expression);
    }

    fn visit_array_expression(&mut self, expression: &ArrayExpression<'a>) {
        self.both("array");
        walk::walk_array_expression(self, expression);
    }

    fn visit_function(&mut self, function: &Function<'a>, flags: oxc_semantic::ScopeFlags) {
        self.both("function");
        self.detail(format!("params:{}", function.params.items.len()));
        walk::walk_function(self, function, flags);
    }

    fn visit_arrow_function_expression(&mut self, arrow: &ArrowFunctionExpression<'a>) {
        self.both("arrow");
        self.detail(format!("params:{}", arrow.params.items.len()));
        walk::walk_arrow_function_expression(self, arrow);
    }

    fn visit_string_literal(&mut self, literal: &StringLiteral<'a>) {
        self.both("str");
        let value = literal.value.as_str();
        if value.chars().count() <= 64 {
            self.detail(format!("{value:?}"));
        }
        if self.facts.strings.len() < 256 {
            self.facts.strings.push(value.to_string());
        }
    }

    fn visit_numeric_literal(&mut self, literal: &NumericLiteral<'a>) {
        self.both("num");
        self.detail(format!("{}", literal.value));
        if self.facts.numbers.len() < 256 {
            self.facts.numbers.push(literal.value);
        }
    }

    fn visit_boolean_literal(&mut self, literal: &BooleanLiteral) {
        self.both("bool");
        self.detail(literal.value.to_string());
    }

    fn visit_null_literal(&mut self, _literal: &NullLiteral) {
        self.both("null");
    }

    fn visit_reg_exp_literal(&mut self, literal: &RegExpLiteral<'a>) {
        self.both("regex");
        self.detail(literal.regex.pattern.text.to_string());
    }

    fn visit_template_literal(&mut self, literal: &TemplateLiteral<'a>) {
        self.both("template");
        walk::walk_template_literal(self, literal);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const RENAMED_A: &str = r#"
        function hashOne(input) {
            var acc = 2166136261;
            for (var i = 0; i < input.length; i++) {
                acc ^= input.charCodeAt(i);
                acc = acc * 16777619;
            }
            return acc >>> 0;
        }
    "#;

    const RENAMED_B: &str = r#"
        function Qz(v) {
            var t = 2166136261;
            for (var n = 0; n < v.length; n++) {
                t ^= v.charCodeAt(n);
                t = t * 16777619;
            }
            return t >>> 0;
        }
    "#;

    const DIFFERENT: &str = r#"
        function other(a, b) {
            if (a > b) {
                return a;
            }
            return b;
        }
    "#;

    fn shape_of(source: &str) -> FunctionShape {
        let index = ShapeIndex::build(source, SourceKind::Script).unwrap();
        index.functions.first().cloned().unwrap()
    }

    #[test]
    fn renaming_everything_does_not_change_the_shape() {
        let left = shape_of(RENAMED_A);
        let right = shape_of(RENAMED_B);

        assert_ne!(left.name, right.name);
        assert_eq!(left.shape.skeleton_hash(), right.shape.skeleton_hash());
        assert_eq!(left.shape.text_hash(), right.shape.text_hash());
        assert_eq!(left.shape.similarity(&right.shape, 5), 1.0);
    }

    #[test]
    fn the_normalised_text_keeps_the_magic_constants_and_the_api_names() {
        let text = shape_of(RENAMED_B).text();

        assert!(text.contains("2166136261"), "{text}");
        assert!(text.contains("16777619"), "{text}");
        assert!(text.contains(".charCodeAt"), "{text}");
    }

    #[test]
    fn the_normalised_text_carries_no_local_names() {
        let text = shape_of(RENAMED_B).text();

        assert!(!text.contains("Qz"), "{text}");
        assert!(!text.split_whitespace().any(|token| token == "v"), "{text}");
    }

    #[test]
    fn different_functions_have_different_shapes() {
        let left = shape_of(RENAMED_A);
        let right = shape_of(DIFFERENT);

        assert_ne!(left.shape.skeleton_hash(), right.shape.skeleton_hash());
        assert!(left.shape.similarity(&right.shape, 5) < 0.5);
    }

    #[test]
    fn a_small_edit_leaves_most_of_the_shape_intact() {
        let edited = RENAMED_A.replace("return acc >>> 0;", "acc = acc + 1; return acc >>> 0;");
        let similarity = shape_of(RENAMED_A).shape.similarity(&shape_of(&edited).shape, 5);

        assert!(similarity > 0.6, "similarity was {similarity}");
        assert!(similarity < 1.0);
    }

    #[test]
    fn the_facts_carry_the_constants_and_the_api_names() {
        let shape = shape_of(RENAMED_B);

        assert!(shape.has_number(2166136261.0));
        assert!(shape.has_number(16777619.0));
        assert!(!shape.has_number(5.0));
        assert!(shape.facts.properties.contains("charCodeAt"));
        assert_eq!(shape.facts.loops, 1);
    }

    #[test]
    fn object_literal_keys_are_recorded_for_shape_matching() {
        let index = ShapeIndex::build(
            "function build() { return { key: 'a', sources: [1], extra: 2 }; }",
            SourceKind::Script,
        )
        .unwrap();

        let shape = index.get("build").unwrap();
        assert!(shape.has_object_with(&["key".to_string(), "sources".to_string()]));
        assert!(!shape.has_object_with(&["key".to_string(), "missing".to_string()]));
    }

    #[test]
    fn direct_calls_are_recorded_for_call_graph_work() {
        let index = ShapeIndex::build(
            "function outer() { return inner(1) + other.method(2); }",
            SourceKind::Script,
        )
        .unwrap();

        let facts = &index.get("outer").unwrap().facts;
        assert!(facts.calls.contains("inner"));
        assert!(!facts.calls.contains("method"));
        assert!(facts.properties.contains("method"));
    }

    #[test]
    fn arity_and_position_are_recorded() {
        let shape = shape_of(DIFFERENT);
        assert_eq!(shape.params, 2);
        assert!(shape.end > shape.start);
    }

    #[test]
    fn arrow_functions_bound_to_a_name_are_indexed() {
        let index = ShapeIndex::build(
            "const decode = (text) => text.split('').reverse().join('');",
            SourceKind::Script,
        )
        .unwrap();

        assert_eq!(index.names(), vec!["decode".to_string()]);
        assert_eq!(index.get("decode").unwrap().params, 1);
    }

    #[test]
    fn unparseable_source_is_reported() {
        assert!(ShapeIndex::build("function (", SourceKind::Script).is_err());
    }
}
