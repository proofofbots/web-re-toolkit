use std::collections::HashMap;

use oxc_allocator::{GetAllocator, TakeIn, Vec as ArenaVec};
use oxc_ast::ast::*;
use oxc_ast::builder::AstBuilder;
use oxc_ast_visit::{Visit, VisitMut, walk, walk_mut};
use oxc_span::{GetSpan, Span};
use oxc_syntax::operator::{BinaryOperator, LogicalOperator, UnaryOperator};

use crate::eval::is_pure;
use crate::pipeline::PassContext;

#[derive(Debug, Clone, PartialEq)]
pub enum Template {
    Binary(BinaryOperator, usize, usize),
    Logical(LogicalOperator, usize, usize),
    Unary(UnaryOperator, usize),
    Call { callee: usize, arguments: Vec<usize> },
    New { callee: usize, arguments: Vec<usize> },
    Member { object: usize, key: usize },
    Identity(usize),
}

impl Template {
    pub fn arity(&self) -> usize {
        let mut highest = 0usize;
        self.visit_indices(&mut |index| highest = highest.max(index + 1));
        highest
    }

    pub fn order(&self) -> Vec<usize> {
        let mut order = Vec::new();
        self.visit_indices(&mut |index| order.push(index));
        order
    }

    fn visit_indices(&self, sink: &mut impl FnMut(usize)) {
        match self {
            Template::Binary(_, left, right) | Template::Logical(_, left, right) => {
                sink(*left);
                sink(*right);
            }
            Template::Unary(_, index) | Template::Identity(index) => sink(*index),
            Template::Member { object, key } => {
                sink(*object);
                sink(*key);
            }
            Template::Call { callee, arguments } | Template::New { callee, arguments } => {
                sink(*callee);
                for argument in arguments {
                    sink(*argument);
                }
            }
        }
    }

    pub fn is_ordered(&self) -> bool {
        let order = self.order();
        order.windows(2).all(|pair| pair[0] < pair[1])
    }

    pub fn uses_each_once(&self) -> bool {
        let mut order = self.order();
        let length = order.len();
        order.sort_unstable();
        order.dedup();
        order.len() == length
    }
}

pub fn read_template(function: &Function<'_>) -> Option<Template> {
    let body = function.body.as_ref()?;
    if body.statements.len() != 1 {
        return None;
    }

    let Statement::ReturnStatement(statement) = &body.statements[0] else {
        return None;
    };

    let expression = statement.argument.as_ref()?;
    let parameters = parameter_names(&function.params)?;
    template_from(expression, &parameters)
}

pub fn read_arrow_template(arrow: &ArrowFunctionExpression<'_>) -> Option<Template> {
    let parameters = parameter_names(&arrow.params)?;

    if let Some(expression) = arrow.body.as_expression() {
        return template_from(expression, &parameters);
    }

    let body = arrow.body.as_function_body()?;
    if body.statements.len() != 1 {
        return None;
    }

    let Statement::ReturnStatement(statement) = &body.statements[0] else {
        return None;
    };

    template_from(statement.argument.as_ref()?, &parameters)
}

fn parameter_names(params: &FormalParameters<'_>) -> Option<Vec<String>> {
    if params.rest.is_some() {
        return None;
    }

    let mut out = Vec::with_capacity(params.items.len());
    for item in &params.items {
        let BindingPattern::BindingIdentifier(identifier) = &item.pattern else {
            return None;
        };
        out.push(identifier.name.as_str().to_string());
    }

    Some(out)
}

fn index_of(expression: &Expression<'_>, parameters: &[String]) -> Option<usize> {
    let Expression::Identifier(identifier) = expression else {
        return None;
    };
    parameters
        .iter()
        .position(|name| name == identifier.name.as_str())
}

fn template_from(expression: &Expression<'_>, parameters: &[String]) -> Option<Template> {
    match expression {
        Expression::BinaryExpression(binary) => Some(Template::Binary(
            binary.operator,
            index_of(&binary.left, parameters)?,
            index_of(&binary.right, parameters)?,
        )),
        Expression::LogicalExpression(logical) => Some(Template::Logical(
            logical.operator,
            index_of(&logical.left, parameters)?,
            index_of(&logical.right, parameters)?,
        )),
        Expression::UnaryExpression(unary) => Some(Template::Unary(
            unary.operator,
            index_of(&unary.argument, parameters)?,
        )),
        Expression::CallExpression(call) => {
            let callee = index_of(&call.callee, parameters)?;
            let mut arguments = Vec::with_capacity(call.arguments.len());
            for argument in &call.arguments {
                arguments.push(index_of(argument.as_expression()?, parameters)?);
            }
            Some(Template::Call { callee, arguments })
        }
        Expression::NewExpression(call) => {
            let callee = index_of(&call.callee, parameters)?;
            let mut arguments = Vec::with_capacity(call.arguments.len());
            for argument in &call.arguments {
                arguments.push(index_of(argument.as_expression()?, parameters)?);
            }
            Some(Template::New { callee, arguments })
        }
        Expression::ComputedMemberExpression(member) => Some(Template::Member {
            object: index_of(&member.object, parameters)?,
            key: index_of(&member.expression, parameters)?,
        }),
        Expression::Identifier(_) => Some(Template::Identity(index_of(expression, parameters)?)),
        _ => None,
    }
}

#[derive(Default)]
struct WrapperCollector {
    functions: HashMap<String, Template>,
    objects: HashMap<String, HashMap<String, Template>>,
    disqualified: Vec<String>,
}

impl<'a> Visit<'a> for WrapperCollector {
    fn visit_function(&mut self, function: &Function<'a>, flags: oxc_semantic::ScopeFlags) {
        if let Some(identifier) = &function.id {
            if let Some(template) = read_template(function) {
                self.functions
                    .insert(identifier.name.as_str().to_string(), template);
            }
        }
        walk::walk_function(self, function, flags);
    }

    fn visit_variable_declarator(&mut self, declarator: &VariableDeclarator<'a>) {
        walk::walk_variable_declarator(self, declarator);

        let BindingPattern::BindingIdentifier(identifier) = &declarator.id else {
            return;
        };
        let name = identifier.name.as_str().to_string();

        let Some(init) = &declarator.init else { return };

        match init {
            Expression::FunctionExpression(function) => {
                if let Some(template) = read_template(function) {
                    self.functions.insert(name, template);
                }
            }
            Expression::ArrowFunctionExpression(arrow) => {
                if let Some(template) = read_arrow_template(arrow) {
                    self.functions.insert(name, template);
                }
            }
            Expression::ObjectExpression(object) => {
                let mut entries = HashMap::new();
                let mut all_wrappers = !object.properties.is_empty();

                for property in &object.properties {
                    let ObjectPropertyKind::ObjectProperty(entry) = property else {
                        all_wrappers = false;
                        break;
                    };

                    let Some(key) = property_key_text(&entry.key) else {
                        all_wrappers = false;
                        break;
                    };

                    let template = match &entry.value {
                        Expression::FunctionExpression(function) => read_template(function),
                        Expression::ArrowFunctionExpression(arrow) => read_arrow_template(arrow),
                        _ => None,
                    };

                    match template {
                        Some(template) => {
                            entries.insert(key, template);
                        }
                        None => {
                            all_wrappers = false;
                            break;
                        }
                    }
                }

                if all_wrappers {
                    self.objects.insert(name, entries);
                } else {
                    self.disqualified.push(name);
                }
            }
            _ => {}
        }
    }
}

fn property_key_text(key: &PropertyKey<'_>) -> Option<String> {
    match key {
        PropertyKey::StaticIdentifier(identifier) => Some(identifier.name.as_str().to_string()),
        PropertyKey::StringLiteral(literal) => Some(literal.value.as_str().to_string()),
        _ => None,
    }
}

pub fn inline_operator_wrappers<'a>(
    program: &mut Program<'a>,
    ctx: &mut PassContext<'a>,
) -> usize {
    let mut collector = WrapperCollector::default();
    collector.visit_program(program);

    if collector.functions.is_empty() {
        return 0;
    }

    let mut pass = InlineWrappers {
        ctx,
        functions: collector.functions,
        objects: HashMap::new(),
        changed: 0,
    };
    pass.visit_program(program);
    pass.changed
}

pub fn inline_wrapper_objects<'a>(program: &mut Program<'a>, ctx: &mut PassContext<'a>) -> usize {
    let mut collector = WrapperCollector::default();
    collector.visit_program(program);

    for name in &collector.disqualified {
        collector.objects.remove(name);
    }

    if collector.objects.is_empty() {
        return 0;
    }

    let mut pass = InlineWrappers {
        ctx,
        functions: HashMap::new(),
        objects: collector.objects,
        changed: 0,
    };
    pass.visit_program(program);
    pass.changed
}

struct InlineWrappers<'a, 'c> {
    ctx: &'c mut PassContext<'a>,
    functions: HashMap<String, Template>,
    objects: HashMap<String, HashMap<String, Template>>,
    changed: usize,
}

impl<'a, 'c> InlineWrappers<'a, 'c> {
    fn template_for(&self, callee: &Expression<'a>) -> Option<Template> {
        match callee {
            Expression::Identifier(identifier) => {
                self.functions.get(identifier.name.as_str()).cloned()
            }
            Expression::StaticMemberExpression(member) => {
                let Expression::Identifier(object) = &member.object else {
                    return None;
                };
                self.objects
                    .get(object.name.as_str())?
                    .get(member.property.name.as_str())
                    .cloned()
            }
            Expression::ComputedMemberExpression(member) => {
                let Expression::Identifier(object) = &member.object else {
                    return None;
                };
                let Expression::StringLiteral(key) = &member.expression else {
                    return None;
                };
                self.objects
                    .get(object.name.as_str())?
                    .get(key.value.as_str())
                    .cloned()
            }
            _ => None,
        }
    }

    fn expand(
        &mut self,
        template: &Template,
        arguments: &mut [Expression<'a>],
        span: Span,
    ) -> Option<Expression<'a>> {
        let take = |arguments: &mut [Expression<'a>], index: usize, builder: &_| -> Expression<'a> {
            arguments[index].take_in(builder)
        };

        let builder = AstBuilder::new(self.ctx.builder.allocator());

        Some(match template {
            Template::Binary(operator, left, right) => {
                let left = take(arguments, *left, &builder);
                let right = take(arguments, *right, &builder);
                Expression::new_binary_expression(span, left, *operator, right, &builder)
            }
            Template::Logical(operator, left, right) => {
                let left = take(arguments, *left, &builder);
                let right = take(arguments, *right, &builder);
                Expression::new_logical_expression(span, left, *operator, right, &builder)
            }
            Template::Unary(operator, index) => {
                let argument = take(arguments, *index, &builder);
                Expression::new_unary_expression(span, *operator, argument, &builder)
            }
            Template::Identity(index) => take(arguments, *index, &builder),
            Template::Member { object, key } => {
                let object = take(arguments, *object, &builder);
                let key = take(arguments, *key, &builder);
                Expression::new_computed_member_expression(span, object, key, false, &builder)
            }
            Template::Call { callee, arguments: indices } => {
                let callee = take(arguments, *callee, &builder);
                let mut list = ArenaVec::with_capacity_in(indices.len(), &builder);
                for index in indices {
                    list.push(Argument::from(take(arguments, *index, &builder)));
                }
                Expression::new_call_expression(span, callee, None, list, false, &builder)
            }
            Template::New { callee, arguments: indices } => {
                let callee = take(arguments, *callee, &builder);
                let mut list = ArenaVec::with_capacity_in(indices.len(), &builder);
                for index in indices {
                    list.push(Argument::from(take(arguments, *index, &builder)));
                }
                Expression::new_new_expression(span, callee, None, list, &builder)
            }
        })
    }
}

impl<'a, 'c> VisitMut<'a> for InlineWrappers<'a, 'c> {
    fn visit_expression(&mut self, it: &mut Expression<'a>) {
        walk_mut::walk_expression(self, it);

        let Expression::CallExpression(call) = it else {
            return;
        };

        let Some(template) = self.template_for(&call.callee) else {
            return;
        };

        if !template.uses_each_once() {
            return;
        }

        if call.arguments.len() != template.arity() {
            return;
        }

        let mut arguments = Vec::with_capacity(call.arguments.len());
        for argument in call.arguments.iter_mut() {
            let Some(expression) = argument.as_expression_mut() else {
                return;
            };
            arguments.push(expression.take_in(&self.ctx.builder));
        }

        let safe = template.is_ordered() || arguments.iter().all(is_pure);
        if !safe {
            for (index, argument) in call.arguments.iter_mut().enumerate() {
                if let Some(slot) = argument.as_expression_mut() {
                    *slot = arguments[index].take_in(&self.ctx.builder);
                }
            }
            return;
        }

        let span = it.span();
        if let Some(replacement) = self.expand(&template, &mut arguments, span) {
            *it = replacement;
            self.changed += 1;
        }
    }
}

pub fn remove_wrapper_definitions<'a>(
    program: &mut Program<'a>,
    ctx: &mut PassContext<'a>,
) -> usize {
    let mut collector = WrapperCollector::default();
    collector.visit_program(program);

    if collector.functions.is_empty() && collector.objects.is_empty() {
        return 0;
    }

    let mut counter = ReferenceCounter { counts: HashMap::new() };
    counter.visit_program(program);

    let mut removable: Vec<String> = Vec::new();
    for name in collector.functions.keys().chain(collector.objects.keys()) {
        if counter.counts.get(name).copied().unwrap_or(0) == 0 {
            removable.push(name.clone());
        }
    }

    if removable.is_empty() {
        return 0;
    }

    let mut pass = RemoveDefinitions { ctx, names: removable, changed: 0 };
    pass.visit_program(program);
    pass.changed
}

struct ReferenceCounter {
    counts: HashMap<String, usize>,
}

impl<'a> Visit<'a> for ReferenceCounter {
    fn visit_identifier_reference(&mut self, identifier: &IdentifierReference<'a>) {
        *self
            .counts
            .entry(identifier.name.as_str().to_string())
            .or_insert(0) += 1;
    }
}

struct RemoveDefinitions<'a, 'c> {
    ctx: &'c mut PassContext<'a>,
    names: Vec<String>,
    changed: usize,
}

impl<'a, 'c> VisitMut<'a> for RemoveDefinitions<'a, 'c> {
    fn visit_statements(&mut self, it: &mut ArenaVec<'a, Statement<'a>>) {
        walk_mut::walk_statements(self, it);

        let before = it.len();
        let names = &self.names;

        it.retain(|statement| match statement {
            Statement::FunctionDeclaration(function) => function
                .id
                .as_ref()
                .map(|identifier| !names.iter().any(|name| name == identifier.name.as_str()))
                .unwrap_or(true),
            Statement::VariableDeclaration(declaration) => {
                !declaration.declarations.iter().all(|declarator| {
                    match &declarator.id {
                        BindingPattern::BindingIdentifier(identifier) => {
                            names.iter().any(|name| name == identifier.name.as_str())
                        }
                        _ => false,
                    }
                })
            }
            _ => true,
        });

        self.changed += before - it.len();
        let _ = &self.ctx;
    }
}
