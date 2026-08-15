use oxc_allocator::GetAllocator;
use oxc_ast::ast::*;
use oxc_ast::builder::AstBuilder;
use oxc_span::Span;
use oxc_syntax::number::NumberBase;
use oxc_syntax::operator::{BinaryOperator, LogicalOperator, UnaryOperator};

#[derive(Debug, Clone, PartialEq)]
pub enum Const {
    Number(f64),
    Text(String),
    Bool(bool),
    Null,
    Undefined,
}

impl Const {
    pub fn truthy(&self) -> bool {
        match self {
            Const::Number(value) => *value != 0.0 && !value.is_nan(),
            Const::Text(value) => !value.is_empty(),
            Const::Bool(value) => *value,
            Const::Null | Const::Undefined => false,
        }
    }

    pub fn to_number(&self) -> f64 {
        match self {
            Const::Number(value) => *value,
            Const::Text(value) => {
                let trimmed = value.trim();
                if trimmed.is_empty() {
                    0.0
                } else {
                    trimmed.parse::<f64>().unwrap_or(f64::NAN)
                }
            }
            Const::Bool(value) => {
                if *value {
                    1.0
                } else {
                    0.0
                }
            }
            Const::Null => 0.0,
            Const::Undefined => f64::NAN,
        }
    }

    pub fn to_text(&self) -> String {
        match self {
            Const::Number(value) => format_number(*value),
            Const::Text(value) => value.clone(),
            Const::Bool(value) => value.to_string(),
            Const::Null => "null".to_string(),
            Const::Undefined => "undefined".to_string(),
        }
    }

    pub fn type_name(&self) -> &'static str {
        match self {
            Const::Number(_) => "number",
            Const::Text(_) => "string",
            Const::Bool(_) => "boolean",
            Const::Null => "object",
            Const::Undefined => "undefined",
        }
    }
}

pub fn format_number(value: f64) -> String {
    if value.is_nan() {
        return "NaN".to_string();
    }
    if value.is_infinite() {
        return if value > 0.0 { "Infinity".into() } else { "-Infinity".into() };
    }
    if value == value.trunc() && value.abs() < 1e21 {
        return format!("{}", value as i64);
    }
    let mut text = format!("{value}");
    if text.contains('e') {
        text = text.replace("e", "e+").replace("e+-", "e-");
    }
    text
}

pub fn eval(expression: &Expression<'_>) -> Option<Const> {
    match expression {
        Expression::NumericLiteral(literal) => Some(Const::Number(literal.value)),
        Expression::StringLiteral(literal) => Some(Const::Text(literal.value.as_str().to_string())),
        Expression::BooleanLiteral(literal) => Some(Const::Bool(literal.value)),
        Expression::NullLiteral(_) => Some(Const::Null),
        Expression::Identifier(identifier) => match identifier.name.as_str() {
            "undefined" => Some(Const::Undefined),
            "NaN" => Some(Const::Number(f64::NAN)),
            "Infinity" => Some(Const::Number(f64::INFINITY)),
            _ => None,
        },
        Expression::TemplateLiteral(template) if template.expressions.is_empty() => {
            let quasi = template.quasis.first()?;
            Some(Const::Text(quasi.value.cooked.as_ref()?.as_str().to_string()))
        }
        Expression::UnaryExpression(unary) => {
            let inner = eval(&unary.argument);
            match unary.operator {
                UnaryOperator::LogicalNot => Some(Const::Bool(!inner?.truthy())),
                UnaryOperator::UnaryNegation => Some(Const::Number(-inner?.to_number())),
                UnaryOperator::UnaryPlus => Some(Const::Number(inner?.to_number())),
                UnaryOperator::BitwiseNot => {
                    Some(Const::Number(f64::from(!to_int32(inner?.to_number()))))
                }
                UnaryOperator::Void => Some(Const::Undefined),
                UnaryOperator::Typeof => Some(Const::Text(inner?.type_name().to_string())),
                UnaryOperator::Delete => None,
            }
        }
        Expression::BinaryExpression(binary) => {
            let left = eval(&binary.left)?;
            let right = eval(&binary.right)?;
            eval_binary(binary.operator, &left, &right)
        }
        Expression::LogicalExpression(logical) => {
            let left = eval(&logical.left)?;
            match logical.operator {
                LogicalOperator::And => {
                    if left.truthy() {
                        eval(&logical.right)
                    } else {
                        Some(left)
                    }
                }
                LogicalOperator::Or => {
                    if left.truthy() {
                        Some(left)
                    } else {
                        eval(&logical.right)
                    }
                }
                LogicalOperator::Coalesce => {
                    if matches!(left, Const::Null | Const::Undefined) {
                        eval(&logical.right)
                    } else {
                        Some(left)
                    }
                }
            }
        }
        Expression::ConditionalExpression(conditional) => {
            let test = eval(&conditional.test)?;
            if test.truthy() {
                eval(&conditional.consequent)
            } else {
                eval(&conditional.alternate)
            }
        }
        Expression::ParenthesizedExpression(parenthesized) => eval(&parenthesized.expression),
        Expression::SequenceExpression(sequence) => {
            let last = sequence.expressions.last()?;
            if sequence
                .expressions
                .iter()
                .take(sequence.expressions.len() - 1)
                .all(|item| eval(item).is_some())
            {
                eval(last)
            } else {
                None
            }
        }
        _ => None,
    }
}

fn eval_binary(operator: BinaryOperator, left: &Const, right: &Const) -> Option<Const> {
    use BinaryOperator::*;

    let value = match operator {
        Addition => match (left, right) {
            (Const::Text(_), _) | (_, Const::Text(_)) => {
                Const::Text(format!("{}{}", left.to_text(), right.to_text()))
            }
            _ => Const::Number(left.to_number() + right.to_number()),
        },
        Subtraction => Const::Number(left.to_number() - right.to_number()),
        Multiplication => Const::Number(left.to_number() * right.to_number()),
        Division => Const::Number(left.to_number() / right.to_number()),
        Remainder => Const::Number(left.to_number() % right.to_number()),
        Exponential => Const::Number(left.to_number().powf(right.to_number())),
        BitwiseAnd => Const::Number(f64::from(to_int32(left.to_number()) & to_int32(right.to_number()))),
        BitwiseOR => Const::Number(f64::from(to_int32(left.to_number()) | to_int32(right.to_number()))),
        BitwiseXOR => Const::Number(f64::from(to_int32(left.to_number()) ^ to_int32(right.to_number()))),
        ShiftLeft => Const::Number(f64::from(
            to_int32(left.to_number()).wrapping_shl(to_uint32(right.to_number()) & 31),
        )),
        ShiftRight => Const::Number(f64::from(
            to_int32(left.to_number()).wrapping_shr(to_uint32(right.to_number()) & 31),
        )),
        ShiftRightZeroFill => Const::Number(f64::from(
            to_uint32(left.to_number()).wrapping_shr(to_uint32(right.to_number()) & 31),
        )),
        StrictEquality => Const::Bool(strict_equal(left, right)),
        StrictInequality => Const::Bool(!strict_equal(left, right)),
        Equality => Const::Bool(loose_equal(left, right)),
        Inequality => Const::Bool(!loose_equal(left, right)),
        LessThan => Const::Bool(compare(left, right, |ordering| ordering.is_lt())?),
        LessEqualThan => Const::Bool(compare(left, right, |ordering| ordering.is_le())?),
        GreaterThan => Const::Bool(compare(left, right, |ordering| ordering.is_gt())?),
        GreaterEqualThan => Const::Bool(compare(left, right, |ordering| ordering.is_ge())?),
        In | Instanceof => return None,
    };

    Some(value)
}

fn compare(left: &Const, right: &Const, decide: impl Fn(std::cmp::Ordering) -> bool) -> Option<bool> {
    if let (Const::Text(left), Const::Text(right)) = (left, right) {
        return Some(decide(left.cmp(right)));
    }

    let left = left.to_number();
    let right = right.to_number();
    if left.is_nan() || right.is_nan() {
        return Some(false);
    }
    Some(decide(left.partial_cmp(&right)?))
}

fn strict_equal(left: &Const, right: &Const) -> bool {
    match (left, right) {
        (Const::Number(a), Const::Number(b)) => a == b,
        (Const::Text(a), Const::Text(b)) => a == b,
        (Const::Bool(a), Const::Bool(b)) => a == b,
        (Const::Null, Const::Null) => true,
        (Const::Undefined, Const::Undefined) => true,
        _ => false,
    }
}

fn loose_equal(left: &Const, right: &Const) -> bool {
    match (left, right) {
        (Const::Null | Const::Undefined, Const::Null | Const::Undefined) => true,
        (Const::Null | Const::Undefined, _) | (_, Const::Null | Const::Undefined) => false,
        (Const::Text(a), Const::Text(b)) => a == b,
        _ => {
            let a = left.to_number();
            let b = right.to_number();
            !a.is_nan() && !b.is_nan() && a == b
        }
    }
}

pub fn to_int32(value: f64) -> i32 {
    if !value.is_finite() || value == 0.0 {
        return 0;
    }
    let truncated = value.trunc();
    let wrapped = truncated.rem_euclid(4294967296.0);
    let unsigned = wrapped as u32;
    unsigned as i32
}

pub fn to_uint32(value: f64) -> u32 {
    to_int32(value) as u32
}

pub fn to_expression<'a>(
    value: &Const,
    span: Span,
    builder: &AstBuilder<'a>,
) -> Option<Expression<'a>> {
    let expression = match value {
        Const::Bool(inner) => Expression::new_boolean_literal(span, *inner, builder),
        Const::Null => Expression::new_null_literal(span, builder),
        Const::Undefined => Expression::new_identifier(span, "undefined", builder),
        Const::Number(inner) => {
            if inner.is_nan() {
                Expression::new_identifier(span, "NaN", builder)
            } else if inner.is_infinite() {
                let base = Expression::new_identifier(span, "Infinity", builder);
                if *inner > 0.0 {
                    base
                } else {
                    Expression::new_unary_expression(
                        span,
                        UnaryOperator::UnaryNegation,
                        base,
                        builder,
                    )
                }
            } else if *inner < 0.0 {
                let positive = Expression::new_numeric_literal(
                    span,
                    -*inner,
                    None,
                    number_base(-*inner),
                    builder,
                );
                Expression::new_unary_expression(
                    span,
                    UnaryOperator::UnaryNegation,
                    positive,
                    builder,
                )
            } else {
                Expression::new_numeric_literal(span, *inner, None, number_base(*inner), builder)
            }
        }
        Const::Text(inner) => {
            let arena: &'a str = builder.allocator().alloc_str(inner);
            Expression::new_string_literal(span, arena, None, builder)
        }
    };

    Some(expression)
}

fn number_base(value: f64) -> NumberBase {
    if value == value.trunc() && value.abs() < 9e15 {
        NumberBase::Decimal
    } else {
        NumberBase::Float
    }
}

pub fn is_pure(expression: &Expression<'_>) -> bool {
    match expression {
        Expression::NumericLiteral(_)
        | Expression::StringLiteral(_)
        | Expression::BooleanLiteral(_)
        | Expression::NullLiteral(_)
        | Expression::BigIntLiteral(_)
        | Expression::RegExpLiteral(_)
        | Expression::Identifier(_)
        | Expression::ThisExpression(_)
        | Expression::FunctionExpression(_)
        | Expression::ArrowFunctionExpression(_)
        | Expression::ClassExpression(_) => true,
        Expression::ParenthesizedExpression(inner) => is_pure(&inner.expression),
        Expression::UnaryExpression(unary) => {
            !matches!(unary.operator, UnaryOperator::Delete) && is_pure(&unary.argument)
        }
        Expression::BinaryExpression(binary) => is_pure(&binary.left) && is_pure(&binary.right),
        Expression::LogicalExpression(logical) => is_pure(&logical.left) && is_pure(&logical.right),
        Expression::ConditionalExpression(conditional) => {
            is_pure(&conditional.test)
                && is_pure(&conditional.consequent)
                && is_pure(&conditional.alternate)
        }
        Expression::SequenceExpression(sequence) => sequence.expressions.iter().all(is_pure),
        Expression::ArrayExpression(array) => array.elements.iter().all(|element| match element {
            ArrayExpressionElement::SpreadElement(_) => false,
            other => other.as_expression().map(is_pure).unwrap_or(false),
        }),
        Expression::ObjectExpression(object) => object.properties.iter().all(|property| {
            matches!(property, ObjectPropertyKind::ObjectProperty(entry) if is_pure(&entry.value))
        }),
        Expression::TemplateLiteral(template) => template.expressions.iter().all(is_pure),
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats_numbers_like_javascript() {
        assert_eq!(format_number(1.0), "1");
        assert_eq!(format_number(-0.0), "0");
        assert_eq!(format_number(1.5), "1.5");
        assert_eq!(format_number(f64::NAN), "NaN");
        assert_eq!(format_number(f64::INFINITY), "Infinity");
    }

    #[test]
    fn int32_wraps_like_javascript() {
        assert_eq!(to_int32(4294967296.0), 0);
        assert_eq!(to_int32(-1.0), -1);
        assert_eq!(to_int32(2147483648.0), i32::MIN);
        assert_eq!(to_uint32(-1.0), 4294967295);
    }
}
