use oxc_allocator::TakeIn;
use oxc_ast::ast::*;
use oxc_ast_visit::{VisitMut, walk_mut};
use oxc_span::{GetSpan, Span};

use crate::eval::{Const, eval, to_expression};
use crate::passes::simplify::is_identifier_name;
use crate::pipeline::PassContext;

pub fn call_key(name: &str, arguments: &[Const]) -> String {
    let rendered: Vec<String> = arguments
        .iter()
        .map(|argument| match argument {
            Const::Number(value) => format!("n:{value}"),
            Const::Text(value) => format!("s:{value}"),
            Const::Bool(value) => format!("b:{value}"),
            Const::Null => "null".to_string(),
            Const::Undefined => "undefined".to_string(),
        })
        .collect();

    format!("{name}({})", rendered.join(","))
}

pub fn apply_call_table<'a>(program: &mut Program<'a>, ctx: &mut PassContext<'a>) -> usize {
    if ctx.config.call_values.is_empty() && ctx.config.thunk_values.is_empty() {
        return 0;
    }

    let mut pass = ApplyCallTable { ctx, changed: 0 };
    pass.visit_program(program);
    pass.changed
}

struct ApplyCallTable<'a, 'c> {
    ctx: &'c mut PassContext<'a>,
    changed: usize,
}

impl<'a, 'c> VisitMut<'a> for ApplyCallTable<'a, 'c> {
    fn visit_expression(&mut self, it: &mut Expression<'a>) {
        walk_mut::walk_expression(self, it);

        let Expression::CallExpression(call) = it else {
            return;
        };

        let Expression::Identifier(identifier) = &call.callee else {
            return;
        };

        let name = identifier.name.as_str().to_string();

        let mut arguments = Vec::with_capacity(call.arguments.len());
        let mut all_const = true;

        for argument in &call.arguments {
            match argument.as_expression().and_then(eval) {
                Some(value) => arguments.push(value),
                None => {
                    all_const = false;
                    break;
                }
            }
        }

        let resolved = if all_const {
            let key = call_key(&name, &arguments);
            self.ctx
                .config
                .call_values
                .get(&key)
                .cloned()
                .or_else(|| self.ctx.config.thunk_values.get(&name).cloned())
        } else {
            self.ctx.config.thunk_values.get(&name).cloned()
        };

        let Some(value) = resolved else { return };

        let span = it.span();
        if let Some(replacement) = to_expression(&value, span, &self.ctx.builder) {
            *it = replacement;
            self.changed += 1;
        }
    }
}

pub fn inline_index_tables<'a>(program: &mut Program<'a>, ctx: &mut PassContext<'a>) -> usize {
    if ctx.config.index_tables.is_empty() {
        return 0;
    }

    let mut pass = InlineIndexTables { ctx, changed: 0 };
    pass.visit_program(program);
    pass.changed
}

struct InlineIndexTables<'a, 'c> {
    ctx: &'c mut PassContext<'a>,
    changed: usize,
}

impl<'a, 'c> VisitMut<'a> for InlineIndexTables<'a, 'c> {
    fn visit_expression(&mut self, it: &mut Expression<'a>) {
        walk_mut::walk_expression(self, it);

        let Expression::ComputedMemberExpression(member) = it else {
            return;
        };

        let Expression::Identifier(identifier) = &member.object else {
            return;
        };

        let Some(table) = self.ctx.config.index_tables.get(identifier.name.as_str()) else {
            return;
        };

        let Some(Const::Number(index)) = eval(&member.expression) else {
            return;
        };

        if index < 0.0 || index.fract() != 0.0 {
            return;
        }

        let Some(value) = table.get(index as usize).cloned() else {
            return;
        };

        let span = it.span();
        if let Some(replacement) = to_expression(&value, span, &self.ctx.builder) {
            *it = replacement;
            self.changed += 1;
        }
    }
}

pub fn restore_member_reads<'a>(program: &mut Program<'a>, ctx: &mut PassContext<'a>) -> usize {
    if ctx.config.member_reads.is_empty() {
        return 0;
    }

    let aggressive = ctx.config.aggressive_member_access;
    let mut pass = RestoreMemberReads { ctx, changed: 0, aggressive };
    pass.visit_program(program);
    pass.changed
}

struct RestoreMemberReads<'a, 'c> {
    ctx: &'c mut PassContext<'a>,
    changed: usize,
    aggressive: bool,
}

impl<'a, 'c> RestoreMemberReads<'a, 'c> {
    fn collapse(&mut self, expression: &mut Expression<'a>) -> bool {
        let span = expression.span();

        let Expression::CallExpression(call) = expression else {
            return false;
        };

        let Expression::Identifier(identifier) = &call.callee else {
            return false;
        };

        let name = identifier.name.as_str();
        let Some(spec) = self
            .ctx
            .config
            .member_reads
            .iter()
            .find(|spec| spec.function == name)
            .cloned()
        else {
            return false;
        };

        if call.arguments.len() <= spec.object_arg.max(spec.key_arg) {
            return false;
        }

        let Some(Const::Text(key)) = call
            .arguments
            .get(spec.key_arg)
            .and_then(|argument| argument.as_expression())
            .and_then(eval)
        else {
            return false;
        };

        let Some(object_expression) = call
            .arguments
            .get_mut(spec.object_arg)
            .and_then(|argument| argument.as_expression_mut())
        else {
            return false;
        };

        let object = object_expression.take_in(&self.ctx.builder);

        let replacement = if is_identifier_name(&key) {
            let name = self.ctx.alloc(&key);
            let property = IdentifierName::new(Span::default(), name, &self.ctx.builder);
            Expression::new_static_member_expression(
                span,
                object,
                property,
                false,
                &self.ctx.builder,
            )
        } else {
            let name = self.ctx.alloc(&key);
            let key_expression =
                Expression::new_string_literal(Span::default(), name, None, &self.ctx.builder);
            Expression::new_computed_member_expression(
                span,
                object,
                key_expression,
                false,
                &self.ctx.builder,
            )
        };

        *expression = replacement;
        self.changed += 1;
        true
    }
}

impl<'a, 'c> VisitMut<'a> for RestoreMemberReads<'a, 'c> {
    fn visit_expression(&mut self, it: &mut Expression<'a>) {
        walk_mut::walk_expression(self, it);

        if self.aggressive {
            self.collapse(it);
            return;
        }

        if let Expression::CallExpression(outer) = it {
            let mut callee = outer.callee.take_in(&self.ctx.builder);
            if !self.collapse(&mut callee) {
                outer.callee = callee;
                return;
            }
            outer.callee = callee;
        }
    }
}

pub fn resolve_hash_arguments<'a>(program: &mut Program<'a>, ctx: &mut PassContext<'a>) -> usize {
    if ctx.config.hash_names.is_empty() || ctx.config.hash_functions.is_empty() {
        return 0;
    }

    let mut pass = ResolveHashArguments { ctx, changed: 0 };
    pass.visit_program(program);
    pass.changed
}

struct ResolveHashArguments<'a, 'c> {
    ctx: &'c mut PassContext<'a>,
    changed: usize,
}

impl<'a, 'c> VisitMut<'a> for ResolveHashArguments<'a, 'c> {
    fn visit_expression(&mut self, it: &mut Expression<'a>) {
        walk_mut::walk_expression(self, it);

        let Expression::CallExpression(call) = it else {
            return;
        };

        let Expression::Identifier(identifier) = &call.callee else {
            return;
        };

        if !self
            .ctx
            .config
            .hash_functions
            .iter()
            .any(|name| name == identifier.name.as_str())
        {
            return;
        }

        let mut replaced = 0usize;
        let mut updates: Vec<(usize, String)> = Vec::new();

        for (index, argument) in call.arguments.iter().enumerate() {
            let Some(Const::Number(value)) = argument.as_expression().and_then(eval) else {
                continue;
            };

            if value < 0.0 || value > f64::from(u32::MAX) || value.fract() != 0.0 {
                continue;
            }

            if let Some(name) = self.ctx.config.hash_names.get(&(value as u32)) {
                updates.push((index, name.clone()));
            }
        }

        for (index, name) in updates {
            let arena = self.ctx.alloc(&name);
            let replacement =
                Expression::new_string_literal(Span::default(), arena, None, &self.ctx.builder);
            if let Some(argument) = call.arguments.get_mut(index) {
                *argument = Argument::from(replacement);
                replaced += 1;
            }
        }

        self.changed += replaced;
    }
}
