use oxc_allocator::Allocator;
use oxc_ast::ast::*;
use oxc_ast_visit::{Visit, walk};
use oxc_parser::{ParseOptions, Parser};
use serde::{Deserialize, Serialize};

use wre_core::error::{Error, Result};
use wre_js::pipeline::SourceKind;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DispatchCandidate {
    pub callee: String,
    pub arity: usize,
    pub start: u32,
    pub end: u32,
    pub loop_kind: String,
    pub all_identifier_arguments: bool,
    pub score: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TableCandidate {
    pub name: Option<String>,
    pub length: usize,
    pub start: u32,
    pub end: u32,
    pub uniform_arity: Option<usize>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct DiscoveryReport {
    pub dispatch: Vec<DispatchCandidate>,
    pub tables: Vec<TableCandidate>,
    pub loops: usize,
}

impl DiscoveryReport {
    pub fn best_dispatch(&self) -> Option<&DispatchCandidate> {
        self.dispatch.first()
    }

    pub fn largest_table(&self) -> Option<&TableCandidate> {
        self.tables.iter().max_by_key(|table| table.length)
    }
}

pub fn discover(source: &str, kind: SourceKind) -> Result<DiscoveryReport> {
    let allocator = Allocator::default();
    let parsed = Parser::new(&allocator, source, kind.to_source_type())
        .with_options(ParseOptions {
            preserve_parens: false,
            ..ParseOptions::default()
        })
        .parse();

    if parsed.panicked {
        return Err(Error::msg("vm discovery could not parse the source"));
    }

    let mut collector = Collector {
        loop_depth: 0,
        loop_kind: Vec::new(),
        report: DiscoveryReport::default(),
    };

    collector.visit_program(&parsed.program);

    let mut report = collector.report;
    report
        .dispatch
        .sort_by(|left, right| right.score.cmp(&left.score));
    report.tables.sort_by(|left, right| right.length.cmp(&left.length));

    Ok(report)
}

struct Collector {
    loop_depth: usize,
    loop_kind: Vec<&'static str>,
    report: DiscoveryReport,
}

impl Collector {
    fn enter_loop(&mut self, kind: &'static str) {
        self.loop_depth += 1;
        self.loop_kind.push(kind);
        self.report.loops += 1;
    }

    fn leave_loop(&mut self) {
        self.loop_depth = self.loop_depth.saturating_sub(1);
        self.loop_kind.pop();
    }
}

impl<'a> Visit<'a> for Collector {
    fn visit_while_statement(&mut self, statement: &WhileStatement<'a>) {
        self.enter_loop("while");
        walk::walk_while_statement(self, statement);
        self.leave_loop();
    }

    fn visit_do_while_statement(&mut self, statement: &DoWhileStatement<'a>) {
        self.enter_loop("do-while");
        walk::walk_do_while_statement(self, statement);
        self.leave_loop();
    }

    fn visit_for_statement(&mut self, statement: &ForStatement<'a>) {
        self.enter_loop("for");
        walk::walk_for_statement(self, statement);
        self.leave_loop();
    }

    fn visit_call_expression(&mut self, call: &CallExpression<'a>) {
        if self.loop_depth > 0 && !call.arguments.is_empty() {
            let callee = match &call.callee {
                Expression::Identifier(identifier) => Some(identifier.name.as_str().to_string()),
                Expression::ComputedMemberExpression(member) => match &member.object {
                    Expression::Identifier(object) => {
                        Some(format!("{}[...]", object.name.as_str()))
                    }
                    _ => None,
                },
                Expression::StaticMemberExpression(member) => Some(format!(
                    "{}.{}",
                    render_object(&member.object),
                    member.property.name.as_str()
                )),
                _ => None,
            };

            if let Some(callee) = callee {
                let all_identifier_arguments = call.arguments.iter().all(|argument| {
                    matches!(argument.as_expression(), Some(Expression::Identifier(_)))
                });

                let arity = call.arguments.len();
                let mut score = 0u32;

                if all_identifier_arguments {
                    score += 40;
                }
                if (4..=8).contains(&arity) {
                    score += 30;
                } else if arity >= 3 {
                    score += 15;
                }
                if callee.contains("[...]") {
                    score += 20;
                }
                if self.loop_kind.last().copied() == Some("while") {
                    score += 10;
                }

                self.report.dispatch.push(DispatchCandidate {
                    callee,
                    arity,
                    start: call.span.start,
                    end: call.span.end,
                    loop_kind: self
                        .loop_kind
                        .last()
                        .copied()
                        .unwrap_or("loop")
                        .to_string(),
                    all_identifier_arguments,
                    score,
                });
            }
        }

        walk::walk_call_expression(self, call);
    }

    fn visit_variable_declarator(&mut self, declarator: &VariableDeclarator<'a>) {
        if let Some(Expression::ArrayExpression(array)) = &declarator.init {
            let functions = array
                .elements
                .iter()
                .filter(|element| {
                    matches!(
                        element.as_expression(),
                        Some(Expression::FunctionExpression(_))
                            | Some(Expression::ArrowFunctionExpression(_))
                    )
                })
                .count();

            if functions >= 4 && functions * 2 >= array.elements.len() {
                let name = match &declarator.id {
                    BindingPattern::BindingIdentifier(identifier) => {
                        Some(identifier.name.as_str().to_string())
                    }
                    _ => None,
                };

                self.report.tables.push(TableCandidate {
                    name,
                    length: array.elements.len(),
                    start: array.span.start,
                    end: array.span.end,
                    uniform_arity: uniform_arity(array),
                });
            }
        }

        walk::walk_variable_declarator(self, declarator);
    }

    fn visit_array_expression(&mut self, array: &ArrayExpression<'a>) {
        let functions = array
            .elements
            .iter()
            .filter(|element| {
                matches!(
                    element.as_expression(),
                    Some(Expression::FunctionExpression(_))
                        | Some(Expression::ArrowFunctionExpression(_))
                )
            })
            .count();

        if functions >= 8
            && !self
                .report
                .tables
                .iter()
                .any(|table| table.start == array.span.start)
        {
            self.report.tables.push(TableCandidate {
                name: None,
                length: array.elements.len(),
                start: array.span.start,
                end: array.span.end,
                uniform_arity: uniform_arity(array),
            });
        }

        walk::walk_array_expression(self, array);
    }
}

fn render_object(expression: &Expression<'_>) -> String {
    match expression {
        Expression::Identifier(identifier) => identifier.name.as_str().to_string(),
        Expression::ThisExpression(_) => "this".to_string(),
        Expression::StaticMemberExpression(member) => format!(
            "{}.{}",
            render_object(&member.object),
            member.property.name.as_str()
        ),
        _ => "?".to_string(),
    }
}

fn uniform_arity(array: &ArrayExpression<'_>) -> Option<usize> {
    let mut arity: Option<usize> = None;

    for element in &array.elements {
        let count = match element.as_expression() {
            Some(Expression::FunctionExpression(function)) => function.params.items.len(),
            Some(Expression::ArrowFunctionExpression(arrow)) => arrow.params.items.len(),
            _ => continue,
        };

        match arity {
            None => arity = Some(count),
            Some(existing) if existing == count => {}
            Some(_) => return None,
        }
    }

    arity
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_a_dispatch_loop() {
        let source = r#"
            var handlers = [
                function (a, b, c, d, e, f) { return 1; },
                function (a, b, c, d, e, f) { return 2; },
                function (a, b, c, d, e, f) { return 3; },
                function (a, b, c, d, e, f) { return 4; },
                function (a, b, c, d, e, f) { return 5; }
            ];
            function run(state, read, store, scope, globals, helpers) {
                while (state.k[0] < globals[2].length) {
                    var op = globals[2][state.k[0]++];
                    handlers[op](state, read, store, scope, globals, helpers);
                }
            }
        "#;

        let report = discover(source, SourceKind::Script).unwrap();
        let best = report.best_dispatch().expect("a dispatch candidate");

        assert_eq!(best.callee, "handlers[...]");
        assert_eq!(best.arity, 6);
        assert!(best.all_identifier_arguments);
        assert_eq!(best.loop_kind, "while");

        let table = report.largest_table().expect("a table candidate");
        assert_eq!(table.length, 5);
        assert_eq!(table.uniform_arity, Some(6));
        assert_eq!(table.name.as_deref(), Some("handlers"));
    }

    #[test]
    fn ignores_source_without_a_loop() {
        let report = discover("var x = f(1, 2, 3);", SourceKind::Script).unwrap();
        assert!(report.dispatch.is_empty());
        assert_eq!(report.loops, 0);
    }
}
