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
    // Collect function definitions with their lexical scope chains first: the
    // object walk needs per-object scope tables for call edges (function
    // scope is PER OBJECT in Yul — the creation and deployed objects
    // routinely define same-named helpers), and the fn units come from the
    // same collection. `assign_fn_paths` then turns each into a stable node
    // path, qualifying only the names that actually recur (see its docs).
    let mut functions = Vec::new();
    collect_functions(object, "o", &mut functions);
    assign_fn_paths(&mut functions);

    let object_graph_key = GraphKey::new(
        EntityKey::new("yul.object", owner, "o").map_err(YulLowerError::Core)?,
        "yul-object",
    )
    .map_err(YulLowerError::Core)?;
    let mut graph = Graph::new(object_graph_key.clone());

    // object path -> the functions visible in that object, each with its
    // enclosing-function scope chain. Function NODES are pre-created so call
    // edges can target them regardless of textual order (Yul allows calls
    // before the definition appears); resolution is by scope chain so a call
    // binds to the definition actually visible at the call site.
    let mut object_scopes: BTreeMap<String, Vec<ScopedFn>> = BTreeMap::new();
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
        object_scopes
            .entry(collected.object_path.clone())
            .or_default()
            .push(ScopedFn {
                chain: collected.chain.clone(),
                name: collected.def.name.clone(),
                key,
            });
    }

    lower_object_scoped(owner, &mut graph, options, object, "o", &object_scopes)?;

    let object_unit = LoweredUnit {
        graph_key: object_graph_key,
        graph,
        unit: "yul-object",
        name: object.name.clone(),
    };

    // One standalone graph per function: same owner, fn-rooted paths. The
    // unit's scope table covers itself at the root (self-recursion stays an
    // SCC) plus any NESTED definitions inside its body, pre-registered like
    // the object walk so forward/sibling calls can edge to them. The function
    // itself is rooted here, so it sits at the empty chain — matching the
    // chain `lower_function` lowers its body under.
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
        let mut nested_chain = Vec::new();
        collect_functions_in_block(&def.body, path, &mut nested_chain, &mut nested);
        assign_fn_paths(&mut nested);

        let self_key = NodeKey::entity(
            EntityKey::new("yul.function", owner, path.as_str()).map_err(YulLowerError::Core)?,
        );
        let mut scopes = vec![ScopedFn {
            chain: Vec::new(),
            name: def.name.clone(),
            key: self_key,
        }];
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
            scopes.push(ScopedFn {
                chain: inner.chain.clone(),
                name: inner.def.name.clone(),
                key,
            });
        }

        let mut ctx = Ctx {
            owner,
            graph: &mut graph,
            scopes: &scopes,
            chain: Vec::new(),
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
    /// Enclosing function names, outermost first — the lexical scope chain
    /// (blocks/ifs/loops do not nest function scope, only functions do).
    chain: Vec<String>,
    /// Node path, filled by `assign_fn_paths` once collisions are known.
    fn_path: String,
    def: &'a FunctionDefinition,
}

/// A function's lexical coordinates within one object scope: its
/// enclosing-function chain plus the pre-created node key. Call resolution
/// walks these by scope (see [`resolve_call`]).
struct ScopedFn {
    chain: Vec<String>,
    name: String,
    key: NodeKey,
}

fn collect_functions<'a>(object: &'a Object, object_path: &str, out: &mut Vec<CollectedFn<'a>>) {
    let mut chain = Vec::new();
    collect_functions_in_block(&object.code.block, object_path, &mut chain, out);
    for (index, child) in object.sub_objects.iter().enumerate() {
        if let ObjectChild::Object(sub) = child {
            // Sub-objects are a fresh function scope (their own creation/
            // deployed code), so the chain restarts empty.
            collect_functions(sub, &format!("{object_path}.{index}"), out);
        }
    }
}

fn collect_functions_in_block<'a>(
    block: &'a Block,
    object_path: &str,
    chain: &mut Vec<String>,
    out: &mut Vec<CollectedFn<'a>>,
) {
    for statement in &block.statements {
        match statement {
            Statement::FunctionDefinition(def) => {
                out.push(CollectedFn {
                    object_path: object_path.to_string(),
                    chain: chain.clone(),
                    fn_path: String::new(),
                    def,
                });
                chain.push(def.name.clone());
                collect_functions_in_block(&def.body, object_path, chain, out);
                chain.pop();
            }
            Statement::Block(inner) => collect_functions_in_block(inner, object_path, chain, out),
            Statement::If(stmt) => collect_functions_in_block(&stmt.body, object_path, chain, out),
            Statement::Switch(stmt) => {
                for case in &stmt.cases {
                    collect_functions_in_block(&case.body, object_path, chain, out);
                }
            }
            Statement::ForLoop(stmt) => {
                collect_functions_in_block(&stmt.pre, object_path, chain, out);
                collect_functions_in_block(&stmt.post, object_path, chain, out);
                collect_functions_in_block(&stmt.body, object_path, chain, out);
            }
            _ => {}
        }
    }
}

/// Assign each collected function its node path.
///
/// A name that is unique within its object keeps the flat
/// `<object_path>/fn:<name>` path. This is the overwhelmingly common case and
/// it is identity-stable: every contract that already lowered gets the exact
/// same node keys (and therefore the same digests) as before.
///
/// A name that RECURS within one object is qualified by its enclosing-function
/// scope chain — `<object_path>/fn:<outer>/fn:<name>`. solc's via-IR pipeline
/// emits this whenever two functions each carry an inline-assembly helper of
/// the same name (Seaport 1.6's `usr$gcd`, nested in two different functions);
/// the flat keying used to collide here and abort the whole ingest.
///
/// Residual (genuinely fails loudly, by design): two same-named functions at
/// the SAME function-nesting depth in sibling anonymous blocks share a chain,
/// so they still collide on `DuplicateNode`. That is invalid to mis-resolve
/// and solc does not emit it; widening the qualifier to full block paths would
/// churn identity digests for no real input, so we stop here.
fn assign_fn_paths(functions: &mut [CollectedFn]) {
    let mut counts: BTreeMap<(&str, &str), usize> = BTreeMap::new();
    for collected in functions.iter() {
        *counts
            .entry((collected.object_path.as_str(), collected.def.name.as_str()))
            .or_default() += 1;
    }
    let recurring: std::collections::BTreeSet<(String, String)> = counts
        .iter()
        .filter(|(_, count)| **count > 1)
        .map(|((object, name), _)| ((*object).to_string(), (*name).to_string()))
        .collect();

    for collected in functions.iter_mut() {
        let recurs =
            recurring.contains(&(collected.object_path.clone(), collected.def.name.clone()));
        if recurs {
            let mut path = collected.object_path.clone();
            for enclosing in &collected.chain {
                path.push_str("/fn:");
                path.push_str(enclosing);
            }
            path.push_str("/fn:");
            path.push_str(&collected.def.name);
            collected.fn_path = path;
        } else {
            collected.fn_path = format!("{}/fn:{}", collected.object_path, collected.def.name);
        }
    }
}

/// Whether `prefix` is a (non-strict) leading sub-chain of `chain`.
fn chain_is_prefix(prefix: &[String], chain: &[String]) -> bool {
    prefix.len() <= chain.len() && prefix.iter().zip(chain).all(|(a, b)| a == b)
}

/// Resolve a CALL to `name` from a site at `chain`: the innermost visible
/// definition (longest enclosing-scope prefix). On valid Yul this binds to
/// the one definition in scope — and for a unique name it agrees with the old
/// flat lookup, since a call only ever names a function it can actually see.
fn resolve_call<'a>(scopes: &'a [ScopedFn], chain: &[String], name: &str) -> Option<&'a NodeKey> {
    scopes
        .iter()
        .filter(|scoped| scoped.name == name && chain_is_prefix(&scoped.chain, chain))
        .max_by_key(|scoped| scoped.chain.len())
        .map(|scoped| &scoped.key)
}

/// Resolve a DEFINITION declared at exactly `chain` (used when lowering the
/// definition itself, to recover the path `assign_fn_paths` gave its node).
fn resolve_def<'a>(scopes: &'a [ScopedFn], chain: &[String], name: &str) -> Option<&'a NodeKey> {
    scopes
        .iter()
        .find(|scoped| scoped.name == name && scoped.chain == chain)
        .map(|scoped| &scoped.key)
}

struct Ctx<'a> {
    owner: &'a str,
    graph: &'a mut Graph,
    /// The functions visible in the current object/function scope.
    scopes: &'a [ScopedFn],
    /// Enclosing-function names of the statement currently being lowered;
    /// pushed/popped by [`lower_statement`] around each function body.
    chain: Vec<String>,
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
    object_scopes: &BTreeMap<String, Vec<ScopedFn>>,
) -> Result<NodeKey, YulLowerError> {
    let scopes = object_scopes.get(path).map(Vec::as_slice).unwrap_or(&[]);
    let node = {
        let mut ctx = Ctx {
            owner,
            graph,
            scopes,
            chain: Vec::new(),
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
                    object_scopes,
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
            // within a block changes only its child ordinal. The path is keyed
            // by lexical scope, so same-named helpers in sibling scopes stay
            // distinct (see `assign_fn_paths`).
            let fn_path = match resolve_def(ctx.scopes, &ctx.chain, &def.name) {
                Some(NodeKey::Entity(entity)) => entity.local().to_string(),
                // Not in the scope table (shouldn't happen with correct
                // collection; kept graceful): statement-local path, no
                // incoming call edges.
                _ => format!("{path}/fn:{}", def.name),
            };
            // The body lowers one scope deeper, so calls inside it resolve
            // against this function's own nested helpers first.
            ctx.chain.push(def.name.clone());
            let result = lower_function(ctx, def, &fn_path);
            ctx.chain.pop();
            result
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
                if let Some(fn_key) = resolve_call(ctx.scopes, &ctx.chain, callee) {
                    // Dependency role: recursion becomes an SCC and gets the
                    // WL treatment (invariant I4). Resolution is by lexical
                    // scope, so the edge binds to the definition visible here.
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parse_object;
    use riff_catalog_core::{
        CyclePolicy, DigestRequest, HashPolicy, Value, ViewMode, digest_graph,
    };

    // Two same-named helpers nested in DIFFERENT parent functions. This is
    // legal Yul (sibling scopes don't shadow), and it is exactly what solc's
    // via-IR pipeline emits when two functions each carry an inline-assembly
    // helper of the same name — Seaport 1.6's `usr$gcd` is the real-world
    // case. The deliberately different bodies (recursive Euclid vs. a bare
    // add) make conflation a visible correctness bug rather than a no-op.
    const REUSED_NESTED_NAME: &str = r#"
    object "C" { code {
        function outer_a(x) -> r {
            function gcd(a, b) -> g {
                switch b
                case 0 { g := a }
                default { g := gcd(b, mod(a, b)) }
            }
            r := gcd(x, 6)
        }
        function outer_b(y) -> s {
            function gcd(p, q) -> h { h := add(p, q) }
            s := gcd(y, 7)
        }
        let z := outer_a(10)
        let w := outer_b(20)
    } }"#;

    fn structure_digest(unit: &LoweredUnit) -> riff_catalog_core::Digest {
        let policy = HashPolicy::new(
            YUL_AST_LEVEL,
            ViewMode::AnonymousShape,
            CyclePolicy::CondenseScc,
        )
        .unwrap();
        *digest_graph(
            &DigestRequest::all_dimensions(unit.graph_key.clone(), policy),
            &unit.graph,
        )
        .unwrap()
        .hashes
        .graph
        .get(Dimension::Structure)
        .unwrap()
    }

    /// Regression for the duplicate-yul-name hard-fail (the Seaport ingest
    /// bug): same-named helpers in different scopes must lower into distinct,
    /// correctly-resolved functions instead of colliding on one node key.
    #[test]
    fn reused_nested_function_name_does_not_collide() {
        let object = parse_object(REUSED_NESTED_NAME).unwrap();
        let lowered = lower_object(&object, "test:reused:ir", &LowerOptions::default())
            .expect("same-named nested helpers must lower without DuplicateNode");

        // Both `gcd` definitions surface as their own yul-fn unit, with
        // distinct keys (the collision used to abort lowering entirely).
        let gcds: Vec<&LoweredUnit> = lowered
            .functions
            .iter()
            .filter(|unit| unit.name == "gcd")
            .collect();
        assert_eq!(gcds.len(), 2, "each gcd definition is its own unit");
        assert_ne!(
            gcds[0].graph_key, gcds[1].graph_key,
            "the two gcd units must carry distinct keys"
        );

        // They are genuinely different functions; merging them would corrupt
        // the fingerprint, so their shapes must stay distinct.
        assert_ne!(
            structure_digest(gcds[0]),
            structure_digest(gcds[1]),
            "distinct-shape helpers must not collapse to one fingerprint"
        );

        // Object graph: all four definitions are represented, and each `gcd`
        // call resolves to the gcd in its OWN scope (not a single shared
        // over-approximation), so BOTH gcd nodes receive a `calls` edge.
        let object_graph = &lowered.object.graph;
        let function_nodes = object_graph
            .nodes
            .values()
            .filter(|node| node.kind.as_str() == "yul.function")
            .count();
        assert_eq!(function_nodes, 4, "outer_a, outer_b, and two gcd");

        let gcd_keys: std::collections::BTreeSet<_> = object_graph
            .nodes
            .values()
            .filter(|node| {
                node.kind.as_str() == "yul.function"
                    && node.fields.iter().any(|field| {
                        field.name.as_str() == "name" && field.value == Value::from("gcd")
                    })
            })
            .map(|node| node.key.clone())
            .collect();
        assert_eq!(gcd_keys.len(), 2, "two distinct gcd nodes in the object graph");

        let called: std::collections::BTreeSet<_> = object_graph
            .edges
            .iter()
            .filter(|edge| edge.label.as_str() == "calls")
            .map(|edge| edge.target.clone())
            .collect();
        for gcd in &gcd_keys {
            assert!(
                called.contains(gcd),
                "each gcd must be the target of a scope-correct call edge"
            );
        }
    }
}
