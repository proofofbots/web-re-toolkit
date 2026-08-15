use oxc_allocator::{CloneIn, GetAllocator, Vec as ArenaVec};
use oxc_ast::ast::*;
use oxc_ast_visit::{VisitMut, walk_mut};
use oxc_syntax::operator::{UnaryOperator, UpdateOperator};

use crate::pipeline::PassContext;

pub fn unflatten_switch_order<'a>(
    program: &mut Program<'a>,
    ctx: &mut PassContext<'a>,
) -> usize {
    let mut pass = UnflattenOrder { ctx, changed: 0 };
    pass.visit_program(program);
    pass.changed
}

struct UnflattenOrder<'a, 'c> {
    ctx: &'c mut PassContext<'a>,
    changed: usize,
}

impl<'a, 'c> VisitMut<'a> for UnflattenOrder<'a, 'c> {
    fn visit_statements(&mut self, it: &mut ArenaVec<'a, Statement<'a>>) {
        walk_mut::walk_statements(self, it);

        let mut index = 0usize;
        while index < it.len() {
            match self.try_at(it, index) {
                Some(replacement) => {
                    let span = replacement.span;
                    let consumed = replacement.consumed;
                    let statements = replacement.statements;

                    let _ = span;
                    it.splice(index..index + consumed, statements);
                    self.changed += 1;
                }
                None => index += 1,
            }
        }
    }
}

struct Replacement<'a> {
    statements: Vec<Statement<'a>>,
    consumed: usize,
    span: oxc_span::Span,
}

impl<'a, 'c> UnflattenOrder<'a, 'c> {
    fn try_at(
        &mut self,
        statements: &ArenaVec<'a, Statement<'a>>,
        index: usize,
    ) -> Option<Replacement<'a>> {
        let (order_name, order, counter_name, declared) = read_order(statements, index)?;

        let loop_index = index + declared;
        let (switch_name, counter_used, cases) = read_dispatch(statements.get(loop_index)?)?;

        if switch_name != order_name || counter_used != counter_name {
            return None;
        }

        let mut out: Vec<Statement<'a>> = Vec::new();
        for key in &order {
            let position = cases.iter().position(|(label, _)| label == key)?;
            let case_index = cases[position].1;

            let Statement::WhileStatement(loop_statement) = statements.get(loop_index)? else {
                return None;
            };
            let Statement::BlockStatement(block) = &loop_statement.body else {
                return None;
            };
            let Statement::SwitchStatement(switch) = block.body.first()? else {
                return None;
            };

            let allocator = self.ctx.builder.allocator();
            let case = switch.cases.get(case_index)?;
            for statement in &case.consequent {
                if matches!(statement, Statement::ContinueStatement(_)) {
                    continue;
                }
                out.push(statement.clone_in(allocator));
            }
        }

        self.ctx.note(format!(
            "unflattened a {} step dispatch driven by {order_name}",
            order.len()
        ));

        Some(Replacement {
            statements: out,
            consumed: declared + 1,
            span: oxc_span::Span::default(),
        })
    }
}

fn read_order<'a>(
    statements: &ArenaVec<'a, Statement<'a>>,
    index: usize,
) -> Option<(String, Vec<String>, String, usize)> {
    let Statement::VariableDeclaration(declaration) = statements.get(index)? else {
        return None;
    };

    let mut order_name = None;
    let mut order = None;
    let mut counter_name = None;

    for declarator in &declaration.declarations {
        let BindingPattern::BindingIdentifier(binding) = &declarator.id else {
            return None;
        };
        let name = binding.name.as_str().to_string();

        match &declarator.init {
            Some(Expression::CallExpression(call)) => {
                let parts = split_literal(call)?;
                order_name = Some(name);
                order = Some(parts);
            }
            Some(Expression::NumericLiteral(literal)) if literal.value == 0.0 => {
                counter_name = Some(name);
            }
            _ => return None,
        }
    }

    let mut consumed = 1;

    if counter_name.is_none() {
        let Statement::VariableDeclaration(next) = statements.get(index + 1)? else {
            return None;
        };
        let declarator = next.declarations.first()?;
        let BindingPattern::BindingIdentifier(binding) = &declarator.id else {
            return None;
        };
        match &declarator.init {
            Some(Expression::NumericLiteral(literal)) if literal.value == 0.0 => {
                counter_name = Some(binding.name.as_str().to_string());
                consumed = 2;
            }
            _ => return None,
        }
    }

    Some((order_name?, order?, counter_name?, consumed))
}

fn split_literal(call: &CallExpression<'_>) -> Option<Vec<String>> {
    let Expression::StaticMemberExpression(member) = &call.callee else {
        return None;
    };
    if member.property.name.as_str() != "split" {
        return None;
    }

    let Expression::StringLiteral(source) = &member.object else {
        return None;
    };

    let argument = call.arguments.first()?.as_expression()?;
    let Expression::StringLiteral(separator) = argument else {
        return None;
    };

    Some(
        source
            .value
            .as_str()
            .split(separator.value.as_str())
            .map(str::to_string)
            .collect(),
    )
}

fn read_dispatch<'a>(statement: &Statement<'a>) -> Option<(String, String, Vec<(String, usize)>)> {
    let Statement::WhileStatement(loop_statement) = statement else {
        return None;
    };
    if !is_always_true(&loop_statement.test) {
        return None;
    }

    let Statement::BlockStatement(block) = &loop_statement.body else {
        return None;
    };

    let Statement::SwitchStatement(switch) = block.body.first()? else {
        return None;
    };

    if block.body.len() > 2 {
        return None;
    }
    if let Some(tail) = block.body.get(1)
        && !matches!(tail, Statement::BreakStatement(_))
    {
        return None;
    }

    let Expression::ComputedMemberExpression(member) = &switch.discriminant else {
        return None;
    };
    let Expression::Identifier(order) = &member.object else {
        return None;
    };
    let Expression::UpdateExpression(update) = &member.expression else {
        return None;
    };
    if update.operator != UpdateOperator::Increment || update.prefix {
        return None;
    }
    let SimpleAssignmentTarget::AssignmentTargetIdentifier(counter) = &update.argument else {
        return None;
    };

    let mut cases = Vec::with_capacity(switch.cases.len());
    for (index, case) in switch.cases.iter().enumerate() {
        let Some(Expression::StringLiteral(label)) = &case.test else {
            return None;
        };
        if !case
            .consequent
            .iter()
            .any(|entry| matches!(entry, Statement::ContinueStatement(_)))
        {
            return None;
        }
        cases.push((label.value.as_str().to_string(), index));
    }

    if cases.is_empty() {
        return None;
    }

    Some((
        order.name.as_str().to_string(),
        counter.name.as_str().to_string(),
        cases,
    ))
}

fn is_always_true(expression: &Expression<'_>) -> bool {
    match expression {
        Expression::BooleanLiteral(literal) => literal.value,
        Expression::UnaryExpression(outer) if outer.operator == UnaryOperator::LogicalNot => {
            match &outer.argument {
                Expression::UnaryExpression(inner)
                    if inner.operator == UnaryOperator::LogicalNot =>
                {
                    matches!(
                        inner.argument,
                        Expression::ArrayExpression(_) | Expression::ObjectExpression(_)
                    )
                }
                _ => false,
            }
        }
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use crate::passes::pipeline_named;
    use crate::pipeline::{Config, SourceKind};

    fn unflatten(source: &str) -> String {
        let mut config = Config::structural();
        config.source_type = SourceKind::Script;
        pipeline_named(&["unflatten-switch-order"])
            .run(source, config)
            .unwrap()
            .code
    }

    #[test]
    fn a_shuffled_dispatch_is_put_back_in_order() {
        let source = r#"
            function run() {
                var order = "2|0|3|1".split("|"), step = 0;
                while (true) {
                    switch (order[step++]) {
                        case "0":
                            second();
                            continue;
                        case "1":
                            fourth();
                            continue;
                        case "2":
                            first();
                            continue;
                        case "3":
                            third();
                            continue;
                    }
                    break;
                }
            }
        "#;

        let out = unflatten(source);

        assert!(!out.contains("switch"), "{out}");
        assert!(!out.contains("order"), "{out}");

        let first = out.find("first()").expect("first missing");
        let second = out.find("second()").expect("second missing");
        let third = out.find("third()").expect("third missing");
        let fourth = out.find("fourth()").expect("fourth missing");

        assert!(first < second && second < third && third < fourth, "{out}");
    }

    #[test]
    fn the_counter_may_be_declared_separately() {
        let source = r#"
            function run() {
                var order = "1|0".split("|");
                var step = 0;
                while (true) {
                    switch (order[step++]) {
                        case "0": last(); continue;
                        case "1": early(); continue;
                    }
                    break;
                }
            }
        "#;

        let out = unflatten(source);
        assert!(out.find("early()").unwrap() < out.find("last()").unwrap(), "{out}");
    }

    #[test]
    fn the_obfuscator_spelling_of_true_is_understood() {
        let source = r#"
            function run() {
                var order = "0".split("|"), step = 0;
                while (!![]) {
                    switch (order[step++]) {
                        case "0": only(); continue;
                    }
                    break;
                }
            }
        "#;

        assert!(!unflatten(source).contains("switch"));
    }

    #[test]
    fn a_case_without_a_continue_is_left_alone() {
        let source = r#"
            function run() {
                var order = "0|1".split("|"), step = 0;
                while (true) {
                    switch (order[step++]) {
                        case "0": a(); continue;
                        case "1": b();
                    }
                    break;
                }
            }
        "#;

        assert!(unflatten(source).contains("switch"));
    }

    #[test]
    fn an_order_entry_with_no_case_is_left_alone() {
        let source = r#"
            function run() {
                var order = "0|9".split("|"), step = 0;
                while (true) {
                    switch (order[step++]) {
                        case "0": a(); continue;
                    }
                    break;
                }
            }
        "#;

        assert!(unflatten(source).contains("switch"));
    }

    #[test]
    fn an_ordinary_switch_is_untouched() {
        let source = r#"
            function run(value) {
                switch (value) {
                    case "a": one(); break;
                    case "b": two(); break;
                }
            }
        "#;

        assert!(unflatten(source).contains("switch"));
    }

    #[test]
    fn a_repeated_step_is_emitted_once_per_mention() {
        let source = r#"
            function run() {
                var order = "0|1|0".split("|"), step = 0;
                while (true) {
                    switch (order[step++]) {
                        case "0": tick(); continue;
                        case "1": tock(); continue;
                    }
                    break;
                }
            }
        "#;

        let out = unflatten(source);
        assert_eq!(out.matches("tick()").count(), 2, "{out}");
        assert_eq!(out.matches("tock()").count(), 1, "{out}");
    }
}
