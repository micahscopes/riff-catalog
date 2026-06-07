//! Level "yul-ast/1": lower the Yul syntax tree into riff-catalog graphs.
//!
//! Dimension assignment (the single source of truth for this level):
//!
//! | element            | effect |
//! |--------------------|--------|
//! | object name        | Names `name` (volatile `_178` id suffixes only perturb Names/identity, never shape) |
//! | node kinds         | Structure (implicit via node kind) |
//! | fn/var/target name | Names `name` on the owning node |
//! | builtin callee     | Structure `builtin` (an opcode is semantics, like bytecode opcodes) |
//! | user callee        | Names `callee` + a Dependency-role `calls` edge to the definition when it is in the same graph — recursion thus forms SCCs for the WL machinery |
//! | literal kind       | Structure `kind` |
//! | literal value      | Constants `value` (canonicalized: see `canon`) |
//! | `:type` suffixes   | Types `type` |
//! | default case       | Structure `default` |
//! | data bytes         | Constants `bytes` (only with `include_data`) |
//!
//! Emitted units (invariant I16, coarse granularity): one `yul-object` graph
//! for the whole object tree, plus one `yul-fn` graph per function definition
//! found anywhere in it. Known v1 limitation, on purpose: inside `yul-fn`
//! unit graphs, calls to *other* user functions stay opaque (arity in
//! Structure, callee in Names) because the callee's definition is outside the
//! graph; the `yul-object` graph is the fully connected one.

use std::collections::BTreeMap;

use riff_catalog_core::{Dimension, EdgeRole, EntityKey, Graph, GraphKey, NodeKey};

use crate::ast::*;
use crate::builtins::is_evm_builtin;
use crate::canon::canon_literal;
use crate::error::YulLowerError;

/// The versioned level string for this lowering (invariant I10).
pub const YUL_AST_LEVEL: &str = "yul-ast/1";

#[derive(Clone, Debug, Default)]
pub struct LowerOptions {
    /// Lower `data` segments too. Off by default: the metadata blob is
    /// volatile by construction and solc's JSON drops data names.
    pub include_data: bool,
}

#[derive(Clone, Debug)]
pub struct LoweredUnit {
    pub graph_key: GraphKey,
    pub graph: Graph,
    /// "yul-object" or "yul-fn".
    pub unit: &'static str,
    /// Object name or function name (display).
    pub name: String,
}

#[derive(Clone, Debug)]
pub struct LoweredYul {
    pub object: LoweredUnit,
    pub functions: Vec<LoweredUnit>,
}

/// Lower an object tree. `owner` identifies the artifact (one source
/// compiled one way) and must be IDENTICAL across conformance paths.
pub fn lower_object(
    object: &Object,
    owner: &str,
    options: &LowerOptions,
) -> Result<LoweredYul, YulLowerError> {
    // Collect function definitions with stable paths first: the object walk
    // needs per-object name -> node-key maps for call edges (function scope
    // is PER OBJECT in Yul — the creation and deployed objects routinely
    // define same-named helpers), and the fn units come from the same
    // collection.
    let mut functions = Vec::new();
    collect_functions(object, "o", &mut functions);

    let object_graph_key = GraphKey::new(
        EntityKey::new("yul.object", owner, "o").map_err(YulLowerError::Core)?,
        "yul-object",
    )
    .map_err(YulLowerError::Core)?;
    let mut graph = Graph::new(object_graph_key.clone());

    // object path -> (fn name -> node key). Function NODES are pre-created
    // so call edges can target them regardless of textual order (Yul allows
    // calls before the definition appears).
    let mut scoped_fn_keys: BTreeMap<String, BTreeMap<String, NodeKey>> = BTreeMap::new();
    for collected in &functions {
        let key = NodeKey::entity(
            EntityKey::new("yul.function", owner, collected.fn_path.as_str())
                .map_err(YulLowerError::Core)?,
        );
        graph
            .add_node(key.clone(), "yul.function")
            .map_err(YulLowerError::Core)?;
        graph
            .add_field(&key, Dimension::Names, "name", collected.def.name.as_str())
            .map_err(YulLowerError::Core)?;
        scoped_fn_keys
            .entry(collected.object_path.clone())
            .or_default()
            .insert(collected.def.name.clone(), key);
    }

    let empty = BTreeMap::new();
    lower_object_scoped(
        owner,
        &mut graph,
        options,
        object,
        "o",
        &scoped_fn_keys,
        &empty,
    )?;

    let object_unit = LoweredUnit {
        graph_key: object_graph_key,
        graph,
        unit: "yul-object",
        name: object.name.clone(),
    };

    // One standalone graph per function: same owner, fn-rooted paths. The
    // unit's name map covers itself (self-recursion stays an SCC) plus any
    // NESTED definitions inside its body, pre-registered like the object
    // walk does so forward/sibling calls can edge to them.
    let mut function_units = Vec::new();
    for collected in &functions {
        let path = &collected.fn_path;
        let def = collected.def;
        let graph_key = GraphKey::new(
            EntityKey::new("yul.function", owner, path.as_str()).map_err(YulLowerError::Core)?,
            "yul-fn",
        )
        .map_err(YulLowerError::Core)?;
        let mut graph = Graph::new(graph_key.clone());

        let mut nested = Vec::new();
        collect_functions_in_block(&def.body, path, &mut nested);
        let mut self_map: BTreeMap<String, NodeKey> = BTreeMap::new();
        let self_key = NodeKey::entity(
            EntityKey::new("yul.function", owner, path.as_str()).map_err(YulLowerError::Core)?,
        );
        self_map.insert(def.name.clone(), self_key);
        for inner in &nested {
            let key = NodeKey::entity(
                EntityKey::new("yul.function", owner, inner.fn_path.as_str())
                    .map_err(YulLowerError::Core)?,
            );
            graph
                .add_node(key.clone(), "yul.function")
                .map_err(YulLowerError::Core)?;
            graph
                .add_field(&key, Dimension::Names, "name", inner.def.name.as_str())
                .map_err(YulLowerError::Core)?;
            self_map.insert(inner.def.name.clone(), key);
        }

        let mut ctx = Ctx {
            owner,
            graph: &mut graph,
            fn_keys: &self_map,
        };
        lower_function(&mut ctx, def, path)?;
        function_units.push(LoweredUnit {
            graph_key,
            graph,
            unit: "yul-fn",
            name: def.name.clone(),
        });
    }

    Ok(LoweredYul {
        object: object_unit,
        functions: function_units,
    })
}

struct CollectedFn<'a> {
    object_path: String,
    fn_path: String,
    def: &'a FunctionDefinition,
}

/// Function names are unique within one object's scope chain (Yul forbids
/// shadowing), so `<object_path>/fn:<name>` is collision-free; same-named
/// helpers in different objects (creation vs deployed — routine in solc
/// output) get distinct object paths.
fn collect_functions<'a>(object: &'a Object, object_path: &str, out: &mut Vec<CollectedFn<'a>>) {
    collect_functions_in_block(&object.code.block, object_path, out);
    for (index, child) in object.sub_objects.iter().enumerate() {
        if let ObjectChild::Object(sub) = child {
            collect_functions(sub, &format!("{object_path}.{index}"), out);
        }
    }
}

fn collect_functions_in_block<'a>(
    block: &'a Block,
    object_path: &str,
    out: &mut Vec<CollectedFn<'a>>,
) {
    for statement in &block.statements {
        match statement {
            Statement::FunctionDefinition(def) => {
                out.push(CollectedFn {
                    object_path: object_path.to_string(),
                    fn_path: format!("{object_path}/fn:{}", def.name),
                    def,
                });
                collect_functions_in_block(&def.body, object_path, out);
            }
            Statement::Block(inner) => collect_functions_in_block(inner, object_path, out),
            Statement::If(stmt) => collect_functions_in_block(&stmt.body, object_path, out),
            Statement::Switch(stmt) => {
                for case in &stmt.cases {
                    collect_functions_in_block(&case.body, object_path, out);
                }
            }
            Statement::ForLoop(stmt) => {
                collect_functions_in_block(&stmt.pre, object_path, out);
                collect_functions_in_block(&stmt.post, object_path, out);
                collect_functions_in_block(&stmt.body, object_path, out);
            }
            _ => {}
        }
    }
}

struct Ctx<'a> {
    owner: &'a str,
    graph: &'a mut Graph,
    fn_keys: &'a BTreeMap<String, NodeKey>,
}

impl Ctx<'_> {
    fn node(&mut self, kind: &str, path: &str) -> Result<NodeKey, YulLowerError> {
        let key =
            NodeKey::entity(EntityKey::new(kind, self.owner, path).map_err(YulLowerError::Core)?);
        self.graph
            .add_node(key.clone(), kind)
            .map_err(YulLowerError::Core)?;
        Ok(key)
    }
}

/// Lower one object with its OWN function scope (Yul function visibility is
/// per object), recursing into subobjects with their scopes.
fn lower_object_scoped(
    owner: &str,
    graph: &mut Graph,
    options: &LowerOptions,
    object: &Object,
    path: &str,
    scoped_fn_keys: &BTreeMap<String, BTreeMap<String, NodeKey>>,
    empty: &BTreeMap<String, NodeKey>,
) -> Result<NodeKey, YulLowerError> {
    let fn_keys = scoped_fn_keys.get(path).unwrap_or(empty);
    let node = {
        let mut ctx = Ctx {
            owner,
            graph,
            fn_keys,
        };
        let node = ctx.node("yul.object", path)?;
        if !object.name.is_empty() {
            ctx.graph
                .add_field(&node, Dimension::Names, "name", object.name.as_str())
                .map_err(YulLowerError::Core)?;
        }
        let code = ctx.node("yul.code", &format!("{path}/code"))?;
        ctx.graph
            .add_child(&node, "code", 0, &code)
            .map_err(YulLowerError::Core)?;
        let body = lower_block(
            &mut ctx,
            &object.code.block,
            &format!("{path}/code/b"),
            path,
        )?;
        ctx.graph
            .add_child(&code, "body", 0, &body)
            .map_err(YulLowerError::Core)?;
        node
    };

    for (index, child) in object.sub_objects.iter().enumerate() {
        let ordinal = (index + 1) as u32;
        match child {
            ObjectChild::Object(sub) => {
                let sub_node = lower_object_scoped(
                    owner,
                    graph,
                    options,
                    sub,
                    &format!("{path}.{index}"),
                    scoped_fn_keys,
                    empty,
                )?;
                graph
                    .add_child(&node, "object", ordinal, &sub_node)
                    .map_err(YulLowerError::Core)?;
            }
            ObjectChild::Data(data) => {
                if options.include_data {
                    let data_key = NodeKey::entity(
                        EntityKey::new("yul.data", owner, format!("{path}.{index}/data"))
                            .map_err(YulLowerError::Core)?,
                    );
                    graph
                        .add_node(data_key.clone(), "yul.data")
                        .map_err(YulLowerError::Core)?;
                    graph
                        .add_field(
                            &data_key,
                            Dimension::Constants,
                            "bytes",
                            data.value.as_str(),
                        )
                        .map_err(YulLowerError::Core)?;
                    graph
                        .add_child(&node, "data", ordinal, &data_key)
                        .map_err(YulLowerError::Core)?;
                }
            }
        }
    }
    Ok(node)
}

/// Lower a function definition rooted at its own stable path. Used both for
/// definitions inside the object walk and for standalone `yul-fn` units.
fn lower_function(
    ctx: &mut Ctx<'_>,
    def: &FunctionDefinition,
    fn_path: &str,
) -> Result<NodeKey, YulLowerError> {
    // The function node may have been pre-created (object graphs pre-register
    // every definition so forward calls can edge to it); create it only when
    // lowering a standalone fn unit.
    let key = NodeKey::entity(
        EntityKey::new("yul.function", ctx.owner, fn_path).map_err(YulLowerError::Core)?,
    );
    if !ctx.graph.nodes.contains_key(&key) {
        ctx.graph
            .add_node(key.clone(), "yul.function")
            .map_err(YulLowerError::Core)?;
        ctx.graph
            .add_field(&key, Dimension::Names, "name", def.name.as_str())
            .map_err(YulLowerError::Core)?;
    }

    let mut ordinal = 0u32;
    for (index, parameter) in def.parameters.iter().enumerate() {
        let param = typed_name(ctx, parameter, &format!("{fn_path}/p:{index}"))?;
        ctx.graph
            .add_child(&key, "param", ordinal, &param)
            .map_err(YulLowerError::Core)?;
        ordinal += 1;
    }
    for (index, ret) in def.return_variables.iter().enumerate() {
        let ret_node = typed_name(ctx, ret, &format!("{fn_path}/r:{index}"))?;
        ctx.graph
            .add_child(&key, "ret", ordinal, &ret_node)
            .map_err(YulLowerError::Core)?;
        ordinal += 1;
    }
    let body = lower_block(ctx, &def.body, &format!("{fn_path}/b"), fn_path)?;
    ctx.graph
        .add_child(&key, "body", ordinal, &body)
        .map_err(YulLowerError::Core)?;
    Ok(key)
}

fn typed_name(ctx: &mut Ctx<'_>, name: &TypedName, path: &str) -> Result<NodeKey, YulLowerError> {
    let node = ctx.node("yul.typed-name", path)?;
    ctx.graph
        .add_field(&node, Dimension::Names, "name", name.name.as_str())
        .map_err(YulLowerError::Core)?;
    if let Some(ty) = &name.ty {
        ctx.graph
            .add_field(&node, Dimension::Types, "type", ty.as_str())
            .map_err(YulLowerError::Core)?;
    }
    Ok(node)
}

fn lower_block(
    ctx: &mut Ctx<'_>,
    block: &Block,
    path: &str,
    object_path_for_fns: &str,
) -> Result<NodeKey, YulLowerError> {
    let node = ctx.node("yul.block", path)?;
    for (index, statement) in block.statements.iter().enumerate() {
        let child_path = format!("{path}.{index}");
        let child = lower_statement(ctx, statement, &child_path, object_path_for_fns)?;
        ctx.graph
            .add_child(&node, "stmt", index as u32, &child)
            .map_err(YulLowerError::Core)?;
    }
    Ok(node)
}

fn lower_statement(
    ctx: &mut Ctx<'_>,
    statement: &Statement,
    path: &str,
    scope: &str,
) -> Result<NodeKey, YulLowerError> {
    match statement {
        Statement::Block(block) => lower_block(ctx, block, &format!("{path}/b"), scope),
        Statement::FunctionDefinition(def) => {
            // Definitions live at their own stable fn path (matching the
            // collection pass), not at the statement path: moving a function
            // within a block changes only its child ordinal.
            let fn_path = match ctx.fn_keys.get(&def.name) {
                Some(NodeKey::Entity(entity)) => entity.local().to_string(),
                // Not in the scope map (shouldn't happen with correct
                // collection; kept graceful): statement-local path, no
                // incoming call edges.
                _ => format!("{path}/fn:{}", def.name),
            };
            lower_function(ctx, def, &fn_path)
        }
        Statement::VariableDeclaration(decl) => {
            let node = ctx.node("yul.let", path)?;
            let mut ordinal = 0u32;
            for (index, variable) in decl.variables.iter().enumerate() {
                let var = typed_name(ctx, variable, &format!("{path}/v:{index}"))?;
                ctx.graph
                    .add_child(&node, "var", ordinal, &var)
                    .map_err(YulLowerError::Core)?;
                ordinal += 1;
            }
            if let Some(value) = &decl.value {
                let value_node = lower_expression(ctx, value, &format!("{path}/e"))?;
                ctx.graph
                    .add_child(&node, "value", ordinal, &value_node)
                    .map_err(YulLowerError::Core)?;
            }
            Ok(node)
        }
        Statement::Assignment(assign) => {
            let node = ctx.node("yul.assign", path)?;
            let mut ordinal = 0u32;
            for (index, target) in assign.variable_names.iter().enumerate() {
                let target_node = ctx.node("yul.ident", &format!("{path}/t:{index}"))?;
                ctx.graph
                    .add_field(&target_node, Dimension::Names, "name", target.name.as_str())
                    .map_err(YulLowerError::Core)?;
                ctx.graph
                    .add_child(&node, "target", ordinal, &target_node)
                    .map_err(YulLowerError::Core)?;
                ordinal += 1;
            }
            let value = lower_expression(ctx, &assign.value, &format!("{path}/e"))?;
            ctx.graph
                .add_child(&node, "value", ordinal, &value)
                .map_err(YulLowerError::Core)?;
            Ok(node)
        }
        Statement::Expression(stmt) => {
            let node = ctx.node("yul.expr-stmt", path)?;
            let expr = lower_expression(ctx, &stmt.expression, &format!("{path}/e"))?;
            ctx.graph
                .add_child(&node, "expr", 0, &expr)
                .map_err(YulLowerError::Core)?;
            Ok(node)
        }
        Statement::If(stmt) => {
            let node = ctx.node("yul.if", path)?;
            let cond = lower_expression(ctx, &stmt.condition, &format!("{path}/cond"))?;
            ctx.graph
                .add_child(&node, "cond", 0, &cond)
                .map_err(YulLowerError::Core)?;
            let body = lower_block(ctx, &stmt.body, &format!("{path}/b"), scope)?;
            ctx.graph
                .add_child(&node, "body", 1, &body)
                .map_err(YulLowerError::Core)?;
            Ok(node)
        }
        Statement::Switch(stmt) => {
            let node = ctx.node("yul.switch", path)?;
            let scrutinee = lower_expression(ctx, &stmt.expression, &format!("{path}/scrutinee"))?;
            ctx.graph
                .add_child(&node, "scrutinee", 0, &scrutinee)
                .map_err(YulLowerError::Core)?;
            for (index, case) in stmt.cases.iter().enumerate() {
                let case_path = format!("{path}/case:{index}");
                let case_node = ctx.node("yul.case", &case_path)?;
                match &case.value {
                    Some(literal) => {
                        let value = lower_literal(ctx, literal, &format!("{case_path}/value"))?;
                        ctx.graph
                            .add_child(&case_node, "value", 0, &value)
                            .map_err(YulLowerError::Core)?;
                    }
                    None => {
                        ctx.graph
                            .add_field(&case_node, Dimension::Structure, "default", true)
                            .map_err(YulLowerError::Core)?;
                    }
                }
                let body = lower_block(ctx, &case.body, &format!("{case_path}/b"), scope)?;
                ctx.graph
                    .add_child(&case_node, "body", 1, &body)
                    .map_err(YulLowerError::Core)?;
                ctx.graph
                    .add_child(&node, "case", (index + 1) as u32, &case_node)
                    .map_err(YulLowerError::Core)?;
            }
            Ok(node)
        }
        Statement::ForLoop(stmt) => {
            let node = ctx.node("yul.for", path)?;
            let pre = lower_block(ctx, &stmt.pre, &format!("{path}/pre"), scope)?;
            ctx.graph
                .add_child(&node, "pre", 0, &pre)
                .map_err(YulLowerError::Core)?;
            let cond = lower_expression(ctx, &stmt.condition, &format!("{path}/cond"))?;
            ctx.graph
                .add_child(&node, "cond", 1, &cond)
                .map_err(YulLowerError::Core)?;
            let post = lower_block(ctx, &stmt.post, &format!("{path}/post"), scope)?;
            ctx.graph
                .add_child(&node, "post", 2, &post)
                .map_err(YulLowerError::Core)?;
            let body = lower_block(ctx, &stmt.body, &format!("{path}/b"), scope)?;
            ctx.graph
                .add_child(&node, "body", 3, &body)
                .map_err(YulLowerError::Core)?;
            Ok(node)
        }
        Statement::Break => ctx.node("yul.break", path),
        Statement::Continue => ctx.node("yul.continue", path),
        Statement::Leave => ctx.node("yul.leave", path),
    }
}

fn lower_expression(
    ctx: &mut Ctx<'_>,
    expression: &Expression,
    path: &str,
) -> Result<NodeKey, YulLowerError> {
    match expression {
        Expression::Literal(literal) => lower_literal(ctx, literal, path),
        Expression::Identifier(identifier) => {
            let node = ctx.node("yul.ident", path)?;
            ctx.graph
                .add_field(&node, Dimension::Names, "name", identifier.name.as_str())
                .map_err(YulLowerError::Core)?;
            Ok(node)
        }
        Expression::FunctionCall(call) => {
            let node = ctx.node("yul.call", path)?;
            let callee = call.function_name.name.as_str();
            if is_evm_builtin(callee) {
                ctx.graph
                    .add_field(&node, Dimension::Structure, "builtin", callee)
                    .map_err(YulLowerError::Core)?;
            } else {
                ctx.graph
                    .add_field(&node, Dimension::Names, "callee", callee)
                    .map_err(YulLowerError::Core)?;
                if let Some(fn_key) = ctx.fn_keys.get(callee) {
                    // Dependency role: recursion becomes an SCC and gets the
                    // WL treatment (invariant I4).
                    ctx.graph
                        .add_edge(&node, "calls", fn_key, EdgeRole::Dependency)
                        .map_err(YulLowerError::Core)?;
                }
            }
            for (index, argument) in call.arguments.iter().enumerate() {
                let arg = lower_expression(ctx, argument, &format!("{path}/a:{index}"))?;
                ctx.graph
                    .add_child(&node, "arg", index as u32, &arg)
                    .map_err(YulLowerError::Core)?;
            }
            Ok(node)
        }
    }
}

fn lower_literal(
    ctx: &mut Ctx<'_>,
    literal: &Literal,
    path: &str,
) -> Result<NodeKey, YulLowerError> {
    let node = ctx.node("yul.lit", path)?;
    let kind = match literal.kind {
        LiteralKind::Number => "number",
        LiteralKind::String => "string",
        LiteralKind::Bool => "bool",
    };
    ctx.graph
        .add_field(&node, Dimension::Structure, "kind", kind)
        .map_err(YulLowerError::Core)?;
    let (value, ty) = canon_literal(literal)?;
    ctx.graph
        .add_field(&node, Dimension::Constants, "value", value)
        .map_err(YulLowerError::Core)?;
    if let Some(ty) = ty {
        ctx.graph
            .add_field(&node, Dimension::Types, "type", ty)
            .map_err(YulLowerError::Core)?;
    }
    Ok(node)
}
