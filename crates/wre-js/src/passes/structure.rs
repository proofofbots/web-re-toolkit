use oxc_allocator::{TakeIn, Vec as ArenaVec};
use oxc_ast::ast::*;
use oxc_ast_visit::{VisitMut, walk_mut};
use oxc_span::{GetSpan, Span};
use oxc_syntax::operator::{LogicalOperator, UnaryOperator};

use crate::pipeline::PassContext;

pub fn ensure_blocks<'a>(program: &mut Program<'a>, ctx: &mut PassContext<'a>) -> usize {
    let mut pass = EnsureBlocks { ctx, changed: 0 };
    pass.visit_program(program);
    pass.changed
}

struct EnsureBlocks<'a, 'c> {
    ctx: &'c mut PassContext<'a>,
    changed: usize,
}

impl<'a, 'c> EnsureBlocks<'a, 'c> {
    fn wrap(&mut self, statement: &mut Statement<'a>) {
        if matches!(statement, Statement::BlockStatement(_) | Statement::EmptyStatement(_)) {
            return;
        }

        let span = statement.span();
        let inner = statement.take_in(&self.ctx.builder);
        let mut body = ArenaVec::with_capacity_in(1, &self.ctx.builder);
        body.push(inner);
        *statement = Statement::new_block_statement(span, body, &self.ctx.builder);
        self.changed += 1;
    }
}

impl<'a, 'c> VisitMut<'a> for EnsureBlocks<'a, 'c> {
    fn visit_if_statement(&mut self, it: &mut IfStatement<'a>) {
        walk_mut::walk_if_statement(self, it);
        self.wrap(&mut it.consequent);
        if let Some(alternate) = &mut it.alternate {
            if !matches!(alternate, Statement::IfStatement(_)) {
                self.wrap(alternate);
            }
        }
    }

    fn visit_for_statement(&mut self, it: &mut ForStatement<'a>) {
        walk_mut::walk_for_statement(self, it);
        self.wrap(&mut it.body);
    }

    fn visit_for_in_statement(&mut self, it: &mut ForInStatement<'a>) {
        walk_mut::walk_for_in_statement(self, it);
        self.wrap(&mut it.body);
    }

    fn visit_for_of_statement(&mut self, it: &mut ForOfStatement<'a>) {
        walk_mut::walk_for_of_statement(self, it);
        self.wrap(&mut it.body);
    }

    fn visit_while_statement(&mut self, it: &mut WhileStatement<'a>) {
        walk_mut::walk_while_statement(self, it);
        self.wrap(&mut it.body);
    }

    fn visit_do_while_statement(&mut self, it: &mut DoWhileStatement<'a>) {
        walk_mut::walk_do_while_statement(self, it);
        self.wrap(&mut it.body);
    }
}

pub fn flatten_sequences<'a>(program: &mut Program<'a>, ctx: &mut PassContext<'a>) -> usize {
    let mut pass = FlattenSequences { ctx, changed: 0 };
    pass.visit_program(program);
    pass.changed
}

struct FlattenSequences<'a, 'c> {
    ctx: &'c mut PassContext<'a>,
    changed: usize,
}

impl<'a, 'c> VisitMut<'a> for FlattenSequences<'a, 'c> {
    fn visit_statements(&mut self, it: &mut ArenaVec<'a, Statement<'a>>) {
        walk_mut::walk_statements(self, it);

        if !it.iter().any(is_sequence_statement) {
            return;
        }

        let mut out = ArenaVec::with_capacity_in(it.len() + 4, &self.ctx.builder);

        for statement in it.drain(..) {
            match statement {
                Statement::ExpressionStatement(mut boxed)
                    if matches!(boxed.expression, Expression::SequenceExpression(_)) =>
                {
                    let span = boxed.span;
                    let Expression::SequenceExpression(sequence) =
                        boxed.expression.take_in(&self.ctx.builder)
                    else {
                        unreachable!("checked above");
                    };

                    for part in sequence.unbox().expressions {
                        out.push(Statement::new_expression_statement(
                            span,
                            part,
                            &self.ctx.builder,
                        ));
                    }
                    self.changed += 1;
                }
                other => out.push(other),
            }
        }

        *it = out;
    }

    fn visit_return_statement(&mut self, it: &mut ReturnStatement<'a>) {
        walk_mut::walk_return_statement(self, it);
    }
}

fn is_sequence_statement(statement: &Statement<'_>) -> bool {
    matches!(
        statement,
        Statement::ExpressionStatement(boxed)
            if matches!(boxed.expression, Expression::SequenceExpression(_))
    )
}

pub fn statementize_control_flow<'a>(
    program: &mut Program<'a>,
    ctx: &mut PassContext<'a>,
) -> usize {
    let mut pass = StatementizeControlFlow { ctx, changed: 0 };
    pass.visit_program(program);
    pass.changed
}

struct StatementizeControlFlow<'a, 'c> {
    ctx: &'c mut PassContext<'a>,
    changed: usize,
}

impl<'a, 'c> VisitMut<'a> for StatementizeControlFlow<'a, 'c> {
    fn visit_statement(&mut self, it: &mut Statement<'a>) {
        walk_mut::walk_statement(self, it);

        let Statement::ExpressionStatement(boxed) = it else {
            return;
        };

        let span = boxed.span;

        let Expression::LogicalExpression(logical) = &mut boxed.expression else {
            return;
        };

        if logical.operator == LogicalOperator::Coalesce {
            return;
        }

        let negate = logical.operator == LogicalOperator::Or;
        let test = logical.left.take_in(&self.ctx.builder);
        let action = logical.right.take_in(&self.ctx.builder);

        let test = if negate {
            Expression::new_unary_expression(
                span,
                UnaryOperator::LogicalNot,
                test,
                &self.ctx.builder,
            )
        } else {
            test
        };

        let mut body = ArenaVec::with_capacity_in(1, &self.ctx.builder);
        body.push(Statement::new_expression_statement(span, action, &self.ctx.builder));
        let consequent = Statement::new_block_statement(span, body, &self.ctx.builder);

        *it = Statement::new_if_statement(span, test, consequent, None, &self.ctx.builder);
        self.changed += 1;
    }
}

pub fn statementize_returns<'a>(program: &mut Program<'a>, ctx: &mut PassContext<'a>) -> usize {
    let mut pass = StatementizeReturns { ctx, changed: 0 };
    pass.visit_program(program);
    pass.changed
}

struct StatementizeReturns<'a, 'c> {
    ctx: &'c mut PassContext<'a>,
    changed: usize,
}

impl<'a, 'c> VisitMut<'a> for StatementizeReturns<'a, 'c> {
    fn visit_statement(&mut self, it: &mut Statement<'a>) {
        walk_mut::walk_statement(self, it);

        let Statement::ReturnStatement(boxed) = it else {
            return;
        };

        let Some(Expression::ConditionalExpression(_)) = &boxed.argument else {
            return;
        };

        let span = boxed.span;
        let Some(argument) = boxed.argument.take() else {
            return;
        };

        let Expression::ConditionalExpression(conditional) = argument else {
            unreachable!("checked above");
        };

        let conditional = conditional.unbox();

        let mut consequent_body = ArenaVec::with_capacity_in(1, &self.ctx.builder);
        consequent_body.push(Statement::new_return_statement(
            span,
            Some(conditional.consequent),
            &self.ctx.builder,
        ));

        let mut alternate_body = ArenaVec::with_capacity_in(1, &self.ctx.builder);
        alternate_body.push(Statement::new_return_statement(
            span,
            Some(conditional.alternate),
            &self.ctx.builder,
        ));

        *it = Statement::new_if_statement(
            span,
            conditional.test,
            Statement::new_block_statement(span, consequent_body, &self.ctx.builder),
            Some(Statement::new_block_statement(
                span,
                alternate_body,
                &self.ctx.builder,
            )),
            &self.ctx.builder,
        );

        self.changed += 1;
    }
}

pub fn split_declarations<'a>(program: &mut Program<'a>, ctx: &mut PassContext<'a>) -> usize {
    let mut pass = SplitDeclarations { ctx, changed: 0 };
    pass.visit_program(program);
    pass.changed
}

struct SplitDeclarations<'a, 'c> {
    ctx: &'c mut PassContext<'a>,
    changed: usize,
}

impl<'a, 'c> VisitMut<'a> for SplitDeclarations<'a, 'c> {
    fn visit_statements(&mut self, it: &mut ArenaVec<'a, Statement<'a>>) {
        walk_mut::walk_statements(self, it);

        let splittable = it.iter().any(|statement| {
            matches!(
                statement,
                Statement::VariableDeclaration(declaration)
                    if declaration.declarations.len() > 1
            )
        });

        if !splittable {
            return;
        }

        let mut out = ArenaVec::with_capacity_in(it.len() + 4, &self.ctx.builder);

        for statement in it.drain(..) {
            match statement {
                Statement::VariableDeclaration(boxed) if boxed.declarations.len() > 1 => {
                    let declaration = boxed.unbox();
                    let span = declaration.span;
                    let kind = declaration.kind;
                    let declare = declaration.declare;

                    for declarator in declaration.declarations {
                        let mut single = ArenaVec::with_capacity_in(1, &self.ctx.builder);
                        single.push(declarator);
                        out.push(Statement::new_variable_declaration(
                            span,
                            kind,
                            single,
                            declare,
                            &self.ctx.builder,
                        ));
                    }
                    self.changed += 1;
                }
                other => out.push(other),
            }
        }

        *it = out;
    }
}

pub fn expand_return_sequences<'a>(program: &mut Program<'a>, ctx: &mut PassContext<'a>) -> usize {
    let mut pass = ExpandReturnSequences { ctx, changed: 0 };
    pass.visit_program(program);
    pass.changed
}

struct ExpandReturnSequences<'a, 'c> {
    ctx: &'c mut PassContext<'a>,
    changed: usize,
}

impl<'a, 'c> VisitMut<'a> for ExpandReturnSequences<'a, 'c> {
    fn visit_statements(&mut self, it: &mut ArenaVec<'a, Statement<'a>>) {
        walk_mut::walk_statements(self, it);

        let expandable = it.iter().any(|statement| match statement {
            Statement::ReturnStatement(boxed) => {
                matches!(&boxed.argument, Some(Expression::SequenceExpression(_)))
            }
            _ => false,
        });

        if !expandable {
            return;
        }

        let mut out = ArenaVec::with_capacity_in(it.len() + 4, &self.ctx.builder);

        for statement in it.drain(..) {
            match statement {
                Statement::ReturnStatement(mut boxed)
                    if matches!(&boxed.argument, Some(Expression::SequenceExpression(_))) =>
                {
                    let span = boxed.span;
                    let Some(Expression::SequenceExpression(sequence)) = boxed.argument.take()
                    else {
                        unreachable!("checked above");
                    };

                    let mut expressions = sequence.unbox().expressions;
                    let last = expressions.pop();

                    for part in expressions {
                        out.push(Statement::new_expression_statement(
                            span,
                            part,
                            &self.ctx.builder,
                        ));
                    }

                    out.push(Statement::new_return_statement(span, last, &self.ctx.builder));
                    self.changed += 1;
                }
                other => out.push(other),
            }
        }

        *it = out;
    }
}

pub fn merge_nested_blocks<'a>(program: &mut Program<'a>, ctx: &mut PassContext<'a>) -> usize {
    let mut pass = MergeBlocks { ctx, changed: 0 };
    pass.visit_program(program);
    pass.changed
}

struct MergeBlocks<'a, 'c> {
    ctx: &'c mut PassContext<'a>,
    changed: usize,
}

impl<'a, 'c> VisitMut<'a> for MergeBlocks<'a, 'c> {
    fn visit_statements(&mut self, it: &mut ArenaVec<'a, Statement<'a>>) {
        walk_mut::walk_statements(self, it);

        let has_plain_block = it.iter().any(|statement| match statement {
            Statement::BlockStatement(block) => !declares_binding(&block.body),
            _ => false,
        });

        if !has_plain_block {
            return;
        }

        let mut out = ArenaVec::with_capacity_in(it.len() + 4, &self.ctx.builder);

        for statement in it.drain(..) {
            match statement {
                Statement::BlockStatement(block) if !declares_binding(&block.body) => {
                    for inner in block.unbox().body {
                        out.push(inner);
                    }
                    self.changed += 1;
                }
                other => out.push(other),
            }
        }

        *it = out;
    }
}

fn declares_binding(statements: &[Statement<'_>]) -> bool {
    statements.iter().any(|statement| {
        matches!(
            statement,
            Statement::VariableDeclaration(_)
                | Statement::FunctionDeclaration(_)
                | Statement::ClassDeclaration(_)
        )
    })
}

pub fn empty_span() -> Span {
    Span::default()
}
