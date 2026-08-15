use oxc_allocator::TakeIn;
use oxc_ast::ast::*;
use oxc_ast_visit::{VisitMut, walk_mut};
use oxc_span::{GetSpan, Span};
use oxc_syntax::operator::{BinaryOperator, LogicalOperator, UnaryOperator};

use crate::eval::{Const, eval, is_pure, to_expression};
use crate::pipeline::PassContext;

pub fn simplify_literals<'a>(program: &mut Program<'a>, ctx: &mut PassContext<'a>) -> usize {
    let mut pass = SimplifyLiterals { ctx, changed: 0 };
    pass.visit_program(program);
    pass.changed
}

struct SimplifyLiterals<'a, 'c> {
    ctx: &'c mut PassContext<'a>,
    changed: usize,
}

impl<'a, 'c> VisitMut<'a> for SimplifyLiterals<'a, 'c> {
    fn visit_expression(&mut self, it: &mut Expression<'a>) {
        walk_mut::walk_expression(self, it);

        let span = it.span();

        if let Expression::UnaryExpression(unary) = it {
            match unary.operator {
                UnaryOperator::LogicalNot => {
                    if let Expression::NumericLiteral(number) = &unary.argument {
                        let value = number.value == 0.0;
                        *it = Expression::new_boolean_literal(span, value, &self.ctx.builder);
                        self.changed += 1;
                        return;
                    }
                    if let Expression::ArrayExpression(array) = &unary.argument {
                        if array.elements.is_empty() {
                            *it = Expression::new_boolean_literal(span, false, &self.ctx.builder);
                            self.changed += 1;
                            return;
                        }
                    }
                    if let Expression::BinaryExpression(inner) = &mut unary.argument {
                        if let Some(flipped) = negate_operator(inner.operator) {
                            let left = inner.left.take_in(&self.ctx.builder);
                            let right = inner.right.take_in(&self.ctx.builder);
                            *it = Expression::new_binary_expression(
                                span,
                                left,
                                flipped,
                                right,
                                &self.ctx.builder,
                            );
                            self.changed += 1;
                            return;
                        }
                    }
                }
                UnaryOperator::Void => {
                    if is_pure(&unary.argument) {
                        *it = Expression::new_identifier(
                            span,
                            "undefined",
                            &self.ctx.builder,
                        );
                        self.changed += 1;
                        return;
                    }
                }
                _ => {}
            }
        }

        if let Expression::BinaryExpression(binary) = it {
            if is_comparison(binary.operator)
                && is_literal(&binary.left)
                && !is_literal(&binary.right)
            {
                let left = binary.left.take_in(&self.ctx.builder);
                let right = binary.right.take_in(&self.ctx.builder);
                let operator = mirror_operator(binary.operator);
                *it = Expression::new_binary_expression(
                    span,
                    right,
                    operator,
                    left,
                    &self.ctx.builder,
                );
                self.changed += 1;
                return;
            }

            if binary.operator == BinaryOperator::Division {
                if let (Expression::NumericLiteral(left), Expression::NumericLiteral(right)) =
                    (&binary.left, &binary.right)
                {
                    if right.value == 0.0 && left.value != 0.0 {
                        let name = if left.value > 0.0 { "Infinity" } else { "-Infinity" };
                        let replacement = if name == "Infinity" {
                            Expression::new_identifier(span, "Infinity", &self.ctx.builder)
                        } else {
                            Expression::new_unary_expression(
                                span,
                                UnaryOperator::UnaryNegation,
                                Expression::new_identifier(
                                    span,
                                    "Infinity",
                                    &self.ctx.builder,
                                ),
                                &self.ctx.builder,
                            )
                        };
                        *it = replacement;
                        self.changed += 1;
                    }
                }
            }
        }
    }
}

fn is_literal(expression: &Expression<'_>) -> bool {
    matches!(
        expression,
        Expression::NumericLiteral(_)
            | Expression::StringLiteral(_)
            | Expression::BooleanLiteral(_)
            | Expression::NullLiteral(_)
            | Expression::BigIntLiteral(_)
    )
}

fn is_comparison(operator: BinaryOperator) -> bool {
    matches!(
        operator,
        BinaryOperator::Equality
            | BinaryOperator::Inequality
            | BinaryOperator::StrictEquality
            | BinaryOperator::StrictInequality
            | BinaryOperator::LessThan
            | BinaryOperator::LessEqualThan
            | BinaryOperator::GreaterThan
            | BinaryOperator::GreaterEqualThan
    )
}

fn mirror_operator(operator: BinaryOperator) -> BinaryOperator {
    match operator {
        BinaryOperator::LessThan => BinaryOperator::GreaterThan,
        BinaryOperator::LessEqualThan => BinaryOperator::GreaterEqualThan,
        BinaryOperator::GreaterThan => BinaryOperator::LessThan,
        BinaryOperator::GreaterEqualThan => BinaryOperator::LessEqualThan,
        other => other,
    }
}

fn negate_operator(operator: BinaryOperator) -> Option<BinaryOperator> {
    Some(match operator {
        BinaryOperator::Equality => BinaryOperator::Inequality,
        BinaryOperator::Inequality => BinaryOperator::Equality,
        BinaryOperator::StrictEquality => BinaryOperator::StrictInequality,
        BinaryOperator::StrictInequality => BinaryOperator::StrictEquality,
        _ => return None,
    })
}

pub fn fold_constants<'a>(program: &mut Program<'a>, ctx: &mut PassContext<'a>) -> usize {
    let mut pass = FoldConstants { ctx, changed: 0 };
    pass.visit_program(program);
    pass.changed
}

struct FoldConstants<'a, 'c> {
    ctx: &'c mut PassContext<'a>,
    changed: usize,
}

impl<'a, 'c> VisitMut<'a> for FoldConstants<'a, 'c> {
    fn visit_expression(&mut self, it: &mut Expression<'a>) {
        walk_mut::walk_expression(self, it);

        let foldable = matches!(
            it,
            Expression::BinaryExpression(_)
                | Expression::UnaryExpression(_)
                | Expression::ConditionalExpression(_)
                | Expression::TemplateLiteral(_)
        );

        if !foldable {
            return;
        }

        let Some(value) = eval(it) else { return };

        if matches!(value, Const::Number(number) if number.is_nan() || number.is_infinite()) {
            if !matches!(it, Expression::BinaryExpression(_)) {
                return;
            }
        }

        if let Const::Text(text) = &value {
            if text.len() > 4096 {
                return;
            }
        }

        let span = it.span();
        if let Some(replacement) = to_expression(&value, span, &self.ctx.builder) {
            *it = replacement;
            self.changed += 1;
        }
    }
}

pub fn decode_base64_literals<'a>(program: &mut Program<'a>, ctx: &mut PassContext<'a>) -> usize {
    let mut pass = DecodeBase64 { ctx, changed: 0 };
    pass.visit_program(program);
    pass.changed
}

struct DecodeBase64<'a, 'c> {
    ctx: &'c mut PassContext<'a>,
    changed: usize,
}

impl<'a, 'c> VisitMut<'a> for DecodeBase64<'a, 'c> {
    fn visit_expression(&mut self, it: &mut Expression<'a>) {
        walk_mut::walk_expression(self, it);

        let Expression::CallExpression(call) = it else {
            return;
        };

        let callee = match &call.callee {
            Expression::Identifier(identifier) => identifier.name.as_str(),
            Expression::StaticMemberExpression(member) => member.property.name.as_str(),
            _ => return,
        };

        if callee != "atob" || call.arguments.len() != 1 {
            return;
        }

        let Some(Expression::StringLiteral(literal)) = call.arguments[0].as_expression() else {
            return;
        };

        use base64::Engine;
        use base64::engine::general_purpose::STANDARD;

        let Ok(bytes) = STANDARD.decode(literal.value.as_str()) else {
            return;
        };

        let decoded: String = bytes.iter().map(|byte| *byte as char).collect();
        let span = it.span();
        let arena = self.ctx.alloc(&decoded);
        *it = Expression::new_string_literal(span, arena, None, &self.ctx.builder);
        self.changed += 1;
    }
}

pub fn normalize_member_access<'a>(program: &mut Program<'a>, ctx: &mut PassContext<'a>) -> usize {
    let mut pass = NormalizeMembers { ctx, changed: 0 };
    pass.visit_program(program);
    pass.changed
}

struct NormalizeMembers<'a, 'c> {
    ctx: &'c mut PassContext<'a>,
    changed: usize,
}

impl<'a, 'c> NormalizeMembers<'a, 'c> {
    fn static_parts(
        &mut self,
        member: &mut ComputedMemberExpression<'a>,
    ) -> Option<(Expression<'a>, IdentifierName<'a>, bool)> {
        let Expression::StringLiteral(key) = &member.expression else {
            return None;
        };

        if !is_identifier_name(key.value.as_str()) {
            return None;
        }

        let name = self.ctx.alloc(key.value.as_str());
        let object = member.object.take_in(&self.ctx.builder);
        let optional = member.optional;
        let property = IdentifierName::new(Span::default(), name, &self.ctx.builder);

        Some((object, property, optional))
    }
}

impl<'a, 'c> VisitMut<'a> for NormalizeMembers<'a, 'c> {
    fn visit_expression(&mut self, it: &mut Expression<'a>) {
        walk_mut::walk_expression(self, it);

        let span = it.span();

        let Expression::ComputedMemberExpression(member) = it else {
            return;
        };

        let Some((object, property, optional)) = self.static_parts(member) else {
            return;
        };

        *it = Expression::new_static_member_expression(
            span,
            object,
            property,
            optional,
            &self.ctx.builder,
        );
        self.changed += 1;
    }

    fn visit_simple_assignment_target(&mut self, it: &mut SimpleAssignmentTarget<'a>) {
        walk_mut::walk_simple_assignment_target(self, it);

        let span = it.span();

        let SimpleAssignmentTarget::ComputedMemberExpression(member) = it else {
            return;
        };

        let Some((object, property, optional)) = self.static_parts(member) else {
            return;
        };

        *it = SimpleAssignmentTarget::new_static_member_expression(
            span,
            object,
            property,
            optional,
            &self.ctx.builder,
        );
        self.changed += 1;
    }
}

pub fn normalize_object_keys<'a>(program: &mut Program<'a>, ctx: &mut PassContext<'a>) -> usize {
    let mut pass = NormalizeKeys { ctx, changed: 0 };
    pass.visit_program(program);
    pass.changed
}

struct NormalizeKeys<'a, 'c> {
    ctx: &'c mut PassContext<'a>,
    changed: usize,
}

impl<'a, 'c> VisitMut<'a> for NormalizeKeys<'a, 'c> {
    fn visit_object_property(&mut self, it: &mut ObjectProperty<'a>) {
        walk_mut::walk_object_property(self, it);

        if !it.computed {
            return;
        }

        let PropertyKey::StringLiteral(literal) = &it.key else {
            return;
        };

        if !is_identifier_name(literal.value.as_str()) {
            return;
        }

        let name = self.ctx.alloc(literal.value.as_str());
        it.key = PropertyKey::new_static_identifier(Span::default(), name, &self.ctx.builder);
        it.computed = false;
        self.changed += 1;
    }
}

pub fn is_identifier_name(text: &str) -> bool {
    if text.is_empty() {
        return false;
    }

    const RESERVED: [&str; 37] = [
        "break", "case", "catch", "class", "const", "continue", "debugger", "default", "delete",
        "do", "else", "enum", "export", "extends", "false", "finally", "for", "function", "if",
        "import", "in", "instanceof", "new", "null", "return", "super", "switch", "this", "throw",
        "true", "try", "typeof", "var", "void", "while", "with", "yield",
    ];

    if RESERVED.contains(&text) {
        return false;
    }

    let mut chars = text.chars();
    let first = chars.next().expect("non empty");
    if !(first.is_ascii_alphabetic() || first == '_' || first == '$') {
        return false;
    }

    chars.all(|ch| ch.is_ascii_alphanumeric() || ch == '_' || ch == '$')
}

pub fn restore_nullish_operators<'a>(
    program: &mut Program<'a>,
    ctx: &mut PassContext<'a>,
) -> usize {
    let mut pass = RestoreNullish { ctx, changed: 0 };
    pass.visit_program(program);
    pass.changed
}

struct RestoreNullish<'a, 'c> {
    ctx: &'c mut PassContext<'a>,
    changed: usize,
}

impl<'a, 'c> VisitMut<'a> for RestoreNullish<'a, 'c> {
    fn visit_expression(&mut self, it: &mut Expression<'a>) {
        walk_mut::walk_expression(self, it);

        let span = it.span();

        let Expression::ConditionalExpression(conditional) = it else {
            return;
        };

        let Some(subject) = nullish_test_subject(&conditional.test) else {
            return;
        };

        let consequent_is_subject = same_identifier(&conditional.consequent, &subject);
        let alternate_is_subject = same_identifier(&conditional.alternate, &subject);

        if alternate_is_subject && !consequent_is_subject {
            let left = conditional.alternate.take_in(&self.ctx.builder);
            let right = conditional.consequent.take_in(&self.ctx.builder);
            *it = Expression::new_logical_expression(
                span,
                left,
                LogicalOperator::Coalesce,
                right,
                &self.ctx.builder,
            );
            self.changed += 1;
            return;
        }

        if consequent_is_subject && !alternate_is_subject {
            let left = conditional.consequent.take_in(&self.ctx.builder);
            let right = conditional.alternate.take_in(&self.ctx.builder);
            *it = Expression::new_logical_expression(
                span,
                left,
                LogicalOperator::Coalesce,
                right,
                &self.ctx.builder,
            );
            self.changed += 1;
        }
    }
}

fn nullish_test_subject(test: &Expression<'_>) -> Option<String> {
    let Expression::LogicalExpression(logical) = test else {
        return None;
    };

    if logical.operator != LogicalOperator::Or {
        return None;
    }

    let left = null_comparison_subject(&logical.left)?;
    let right = null_comparison_subject(&logical.right)?;

    if left.0 != right.0 {
        return None;
    }

    let kinds = (left.1, right.1);
    match kinds {
        (NullKind::Null, NullKind::Undefined) | (NullKind::Undefined, NullKind::Null) => {
            Some(left.0)
        }
        _ => None,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum NullKind {
    Null,
    Undefined,
}

fn null_comparison_subject(expression: &Expression<'_>) -> Option<(String, NullKind)> {
    let Expression::BinaryExpression(binary) = expression else {
        return None;
    };

    if !matches!(
        binary.operator,
        BinaryOperator::StrictEquality | BinaryOperator::Equality
    ) {
        return None;
    }

    let (subject, other) = match (&binary.left, &binary.right) {
        (Expression::Identifier(identifier), other) => (identifier.name.as_str(), other),
        (other, Expression::Identifier(identifier)) => (identifier.name.as_str(), other),
        _ => return None,
    };

    let kind = match other {
        Expression::NullLiteral(_) => NullKind::Null,
        Expression::Identifier(identifier) if identifier.name.as_str() == "undefined" => {
            NullKind::Undefined
        }
        Expression::UnaryExpression(unary) if unary.operator == UnaryOperator::Void => {
            NullKind::Undefined
        }
        _ => return None,
    };

    Some((subject.to_string(), kind))
}

fn same_identifier(expression: &Expression<'_>, name: &str) -> bool {
    matches!(expression, Expression::Identifier(identifier) if identifier.name.as_str() == name)
}

pub fn drop_debugger<'a>(program: &mut Program<'a>, ctx: &mut PassContext<'a>) -> usize {
    if !ctx.config.drop_debugger {
        return 0;
    }

    let mut pass = DropDebugger { changed: 0 };
    pass.visit_program(program);
    pass.changed
}

struct DropDebugger {
    changed: usize,
}

impl<'a> VisitMut<'a> for DropDebugger {
    fn visit_statements(&mut self, it: &mut oxc_allocator::Vec<'a, Statement<'a>>) {
        walk_mut::walk_statements(self, it);

        let before = it.len();
        it.retain(|statement| !matches!(statement, Statement::DebuggerStatement(_)));
        self.changed += before - it.len();
    }
}
