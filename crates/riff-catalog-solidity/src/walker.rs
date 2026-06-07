//! The generic nodeType-driven walker.

use std::collections::BTreeMap;

use riff_catalog_core::{Dimension, EdgeRole, EntityKey, Graph, GraphKey, NodeKey};
use riff_catalog_yul::canon::canon_number;
use serde_json::Value;

use crate::error::SolLowerError;
use crate::profile::{ConstExtract, TypeExtract, spec_for};

#[derive(Clone, Debug, Default)]
pub struct WalkOptions {
    /// Unknown nodeType = error (demos/conformance) vs tagged fallback +
    /// warning (sourcify sweeps over arbitrary real-world contracts).
    pub strict: bool,
}

#[derive(Clone, Debug)]
pub struct LoweredUnit {
    pub graph_key: GraphKey,
    pub graph: Graph,
    /// "sol-unit", "sol-contract" or "sol-fn".
    pub unit: &'static str,
    pub name: String,
}

#[derive(Clone, Debug)]
pub struct LoweredSol {
    pub source_unit: LoweredUnit,
    pub contracts: Vec<LoweredUnit>,
    pub functions: Vec<LoweredUnit>,
    pub warnings: Vec<String>,
}

/// Lower a solc `sources.<name>.ast` SourceUnit. `owner` identifies the
/// artifact and must be stable across recompilations (no solc ids).
pub fn lower_source_unit(
    ast: &Value,
    owner: &str,
    options: &WalkOptions,
) -> Result<LoweredSol, SolLowerError> {
    let node_type = ast
        .get("nodeType")
        .and_then(Value::as_str)
        .ok_or_else(|| SolLowerError::MissingNodeType("<root>".into()))?;
    if node_type != "SourceUnit" {
        return Err(SolLowerError::NotASourceUnit(node_type.to_string()));
    }

    let mut warnings = Vec::new();

    // Whole-unit graph.
    let source_unit = walk_unit(
        ast,
        owner,
        "su",
        ("sol.source-unit", "sol-unit"),
        "sol-unit",
        owner.to_string(),
        options,
        &mut warnings,
    )?;

    // Per-contract and per-function graphs (invariant I16).
    let mut contracts = Vec::new();
    let mut functions = Vec::new();
    let empty = Vec::new();
    let top_nodes = ast.get("nodes").and_then(Value::as_array).unwrap_or(&empty);
    for top in top_nodes {
        match top.get("nodeType").and_then(Value::as_str) {
            Some("ContractDefinition") => {
                let contract_name = top
                    .get("name")
                    .and_then(Value::as_str)
                    .unwrap_or("anonymous")
                    .to_string();
                let contract_path = format!("ct:{contract_name}");
                contracts.push(walk_unit(
                    top,
                    owner,
                    &contract_path,
                    ("sol.contract", "sol-contract"),
                    "sol-contract",
                    contract_name.clone(),
                    options,
                    &mut warnings,
                )?);

                let members = top.get("nodes").and_then(Value::as_array).unwrap_or(&empty);
                for (index, member) in members.iter().enumerate() {
                    let node_type = member.get("nodeType").and_then(Value::as_str);
                    if !matches!(
                        node_type,
                        Some("FunctionDefinition") | Some("ModifierDefinition")
                    ) {
                        continue;
                    }
                    let kind = member
                        .get("kind")
                        .and_then(Value::as_str)
                        .unwrap_or("modifier");
                    let name = member.get("name").and_then(Value::as_str).unwrap_or("");
                    let display = if name.is_empty() { kind } else { name };
                    // Path uses declared name + member ordinal: stable across
                    // recompilation/comments, disambiguates overloads without
                    // signature plumbing.
                    let fn_path = format!("{contract_path}/fn:{kind}:{display}:{index}");
                    functions.push(walk_unit(
                        member,
                        owner,
                        &fn_path,
                        ("sol.function", "sol-fn"),
                        "sol-fn",
                        format!("{contract_name}.{display}"),
                        options,
                        &mut warnings,
                    )?);
                }
            }
            Some("FunctionDefinition") => {
                // free function at file scope
                let name = top.get("name").and_then(Value::as_str).unwrap_or("free");
                let fn_path = format!("fn:free:{name}");
                functions.push(walk_unit(
                    top,
                    owner,
                    &fn_path,
                    ("sol.function", "sol-fn"),
                    "sol-fn",
                    name.to_string(),
                    options,
                    &mut warnings,
                )?);
            }
            _ => {}
        }
    }

    Ok(LoweredSol {
        source_unit,
        contracts,
        functions,
        warnings,
    })
}

#[allow(clippy::too_many_arguments)]
fn walk_unit(
    root: &Value,
    owner: &str,
    root_path: &str,
    (root_kind, graph_local): (&str, &str),
    unit: &'static str,
    name: String,
    options: &WalkOptions,
    warnings: &mut Vec<String>,
) -> Result<LoweredUnit, SolLowerError> {
    let graph_key = GraphKey::new(
        EntityKey::new(root_kind, owner, root_path).map_err(SolLowerError::Core)?,
        graph_local,
    )
    .map_err(SolLowerError::Core)?;
    let mut walker = Walker {
        owner,
        graph: Graph::new(graph_key.clone()),
        strict: options.strict,
        warnings,
        ids: BTreeMap::new(),
        refs: Vec::new(),
    };
    walker.walk(root, root_path)?;

    // referencedDeclaration -> Reference edges, resolved within this graph
    // only (negative/foreign ids carry no edge; the Identifier's Names field
    // already holds the text).
    let refs = std::mem::take(&mut walker.refs);
    for (source, id) in refs {
        if let Some(target) = walker.ids.get(&id) {
            walker
                .graph
                .add_edge(&source, "ref", target, EdgeRole::Reference)
                .map_err(SolLowerError::Core)?;
        }
    }

    Ok(LoweredUnit {
        graph_key,
        graph: walker.graph,
        unit,
        name,
    })
}

struct Walker<'a> {
    owner: &'a str,
    graph: Graph,
    strict: bool,
    warnings: &'a mut Vec<String>,
    /// solc id -> node key: resolution side-table only, never hashed (I15).
    ids: BTreeMap<i64, NodeKey>,
    refs: Vec<(NodeKey, i64)>,
}

impl Walker<'_> {
    fn walk(&mut self, value: &Value, path: &str) -> Result<NodeKey, SolLowerError> {
        let node_type = value
            .get("nodeType")
            .and_then(Value::as_str)
            .ok_or_else(|| SolLowerError::MissingNodeType(path.to_string()))?;

        let spec = match spec_for(node_type) {
            Some(spec) => spec,
            None if self.strict => {
                return Err(SolLowerError::UnknownNodeType(node_type.to_string()));
            }
            None => {
                self.warnings
                    .push(format!("unknown nodeType `{node_type}` at {path}"));
                return self.walk_unknown(value, node_type, path);
            }
        };

        let key = self.add_node(spec.kind, path)?;
        if let Some(id) = value.get("id").and_then(Value::as_i64) {
            self.ids.insert(id, key.clone());
        }
        if let Some(referenced) = value.get("referencedDeclaration").and_then(Value::as_i64) {
            self.refs.push((key.clone(), referenced));
        }

        for field in spec.names {
            match value.get(*field) {
                Some(Value::String(text)) if !text.is_empty() => {
                    self.graph
                        .add_field(&key, Dimension::Names, *field, text.as_str())
                        .map_err(SolLowerError::Core)?;
                }
                Some(Value::Array(items)) => {
                    // e.g. FunctionCall.names (named arguments), order matters
                    for (index, item) in items.iter().enumerate() {
                        if let Some(text) = item.as_str() {
                            self.graph
                                .add_field(&key, Dimension::Names, format!("{field}{index}"), text)
                                .map_err(SolLowerError::Core)?;
                        }
                    }
                }
                _ => {}
            }
        }

        for field in spec.structure {
            if let Some(scalar) = value.get(*field).and_then(scalar_to_string) {
                self.graph
                    .add_field(&key, Dimension::Structure, *field, scalar)
                    .map_err(SolLowerError::Core)?;
            }
        }

        for extract in spec.constants {
            self.extract_constant(&key, value, *extract)?;
        }

        for extract in spec.types {
            let text = match extract {
                TypeExtract::TypeString => value
                    .pointer("/typeDescriptions/typeString")
                    .and_then(Value::as_str),
                TypeExtract::Field(field) => value.get(*field).and_then(Value::as_str),
            };
            if let Some(text) = text {
                if !text.is_empty() {
                    self.graph
                        .add_field(&key, Dimension::Types, "type", text)
                        .map_err(SolLowerError::Core)?;
                }
            }
        }

        let mut ordinal = 0u32;
        for (field, label) in spec.children {
            match value.get(*field) {
                None | Some(Value::Null) => {}
                Some(Value::Array(items)) => {
                    for item in items {
                        match item {
                            Value::Null => {
                                // e.g. `(, b) = f()` tuple holes: arity is
                                // structure, so a placeholder preserves it.
                                let hole =
                                    self.add_node("sol.hole", &format!("{path}.{ordinal}"))?;
                                self.attach(&key, label, ordinal, &hole)?;
                                ordinal += 1;
                            }
                            Value::Object(_) => {
                                let child = self.walk(item, &format!("{path}.{ordinal}"))?;
                                self.attach(&key, label, ordinal, &child)?;
                                ordinal += 1;
                            }
                            _ => {}
                        }
                    }
                }
                Some(child @ Value::Object(_)) => {
                    let child_key = self.walk(child, &format!("{path}.{ordinal}"))?;
                    self.attach(&key, label, ordinal, &child_key)?;
                    ordinal += 1;
                }
                _ => {}
            }
        }

        Ok(key)
    }

    /// Non-strict fallback: tagged kind, every object/array-of-object field
    /// as children in sorted key order, no fields read.
    fn walk_unknown(
        &mut self,
        value: &Value,
        node_type: &str,
        path: &str,
    ) -> Result<NodeKey, SolLowerError> {
        let key = self.add_node("sol.unknown", path)?;
        self.graph
            .add_field(&key, Dimension::Structure, "node_type", node_type)
            .map_err(SolLowerError::Core)?;
        let Some(object) = value.as_object() else {
            return Ok(key);
        };
        // serde_json maps preserve insertion order; sort for determinism.
        let mut ordinal = 0u32;
        let mut fields: Vec<(&String, &Value)> = object.iter().collect();
        fields.sort_by_key(|(name, _)| name.as_str());
        for (field, child) in fields {
            match child {
                Value::Object(map) if map.contains_key("nodeType") => {
                    let child_key = self.walk(child, &format!("{path}.{ordinal}"))?;
                    self.attach(&key, field, ordinal, &child_key)?;
                    ordinal += 1;
                }
                Value::Array(items) => {
                    for item in items {
                        if item
                            .as_object()
                            .is_some_and(|map| map.contains_key("nodeType"))
                        {
                            let child_key = self.walk(item, &format!("{path}.{ordinal}"))?;
                            self.attach(&key, field, ordinal, &child_key)?;
                            ordinal += 1;
                        }
                    }
                }
                _ => {}
            }
        }
        Ok(key)
    }

    fn extract_constant(
        &mut self,
        key: &NodeKey,
        value: &Value,
        extract: ConstExtract,
    ) -> Result<(), SolLowerError> {
        match extract {
            ConstExtract::SolLiteral | ConstExtract::YulLiteral => {
                let kind = value.get("kind").and_then(Value::as_str).unwrap_or("");
                let raw = value.get("value").and_then(Value::as_str);
                let hex = value.get("hexValue").and_then(Value::as_str);
                let canonical = match kind {
                    "number" => raw.map(|raw| {
                        // Scientific notation / subdenominated spellings fall
                        // back to the raw spelling (Structure carries the
                        // subdenomination separately).
                        canon_number(raw).unwrap_or_else(|_| raw.to_string())
                    }),
                    "bool" => raw.map(str::to_string),
                    // string-ish kinds: canonical bytes hex; hexValue is the
                    // escape-free source of truth
                    _ => hex
                        .map(|hex| format!("0x{}", hex.to_lowercase()))
                        .or_else(|| {
                            raw.map(|raw| {
                                let mut out = String::from("0x");
                                for byte in raw.as_bytes() {
                                    out.push_str(&format!("{byte:02x}"));
                                }
                                out
                            })
                        }),
                };
                if let Some(canonical) = canonical {
                    self.graph
                        .add_field(key, Dimension::Constants, "value", canonical)
                        .map_err(SolLowerError::Core)?;
                }
            }
            ConstExtract::PragmaLiterals => {
                if let Some(items) = value.get("literals").and_then(Value::as_array) {
                    let joined = items
                        .iter()
                        .filter_map(Value::as_str)
                        .collect::<Vec<_>>()
                        .join(" ");
                    if !joined.is_empty() {
                        self.graph
                            .add_field(key, Dimension::Constants, "pragma", joined)
                            .map_err(SolLowerError::Core)?;
                    }
                }
            }
        }
        Ok(())
    }

    fn add_node(&mut self, kind: &str, path: &str) -> Result<NodeKey, SolLowerError> {
        let key =
            NodeKey::entity(EntityKey::new(kind, self.owner, path).map_err(SolLowerError::Core)?);
        self.graph
            .add_node(key.clone(), kind)
            .map_err(SolLowerError::Core)?;
        Ok(key)
    }

    fn attach(
        &mut self,
        parent: &NodeKey,
        label: &str,
        ordinal: u32,
        child: &NodeKey,
    ) -> Result<(), SolLowerError> {
        self.graph
            .add_child(parent, label, ordinal, child)
            .map_err(SolLowerError::Core)
    }
}

fn scalar_to_string(value: &Value) -> Option<String> {
    match value {
        Value::String(text) if !text.is_empty() => Some(text.clone()),
        Value::Bool(boolean) => Some(boolean.to_string()),
        Value::Number(number) => Some(number.to_string()),
        _ => None,
    }
}
