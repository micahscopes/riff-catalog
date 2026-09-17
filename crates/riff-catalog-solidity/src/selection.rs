//! Ordered syntax with resolved reference incidence, independent of occurrence IDs.
//! Inferred types and external declaration bodies are not part of this contract.
use std::collections::BTreeMap;

use anyhow::{Context, Result, bail, ensure};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

pub const CONTRACT: &str = "solidity-selected-syntax/1";
pub const MAX_NODES: usize = 100_000;

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum View {
    Names,
    Bindings,
    BindingsIgnoreLiterals,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct Span {
    pub start: usize,
    pub length: usize,
    pub file: i64,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct NormalAst {
    pub contract: String,
    pub view: View,
    pub nodes: Vec<Value>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Selection {
    pub id: i64,
    pub kind: String,
    pub name: Option<String>,
    pub span: Span,
    pub normal: NormalAst,
    pub external_bindings: Vec<i64>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SourceArtifact {
    pub path: String,
    pub source: String,
    pub ast: Value,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SourceMatch {
    pub path: String,
    pub selection: Selection,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SearchResult {
    pub schema: String,
    pub query: Selection,
    pub matches: Vec<SourceMatch>,
    pub unsupported: Vec<Value>,
    pub compared: usize,
    pub excluded_self: usize,
    pub truncated: bool,
}

/// Reference retrieval over bounded AST-node candidates. No bug-pattern predicates.
pub fn search(
    artifacts: &[SourceArtifact],
    query_file: usize,
    start: usize,
    end: usize,
    view: View,
) -> Result<SearchResult> {
    ensure!(artifacts.len() <= 100, "source corpus exceeds 100 files");
    let source = artifacts
        .get(query_file)
        .context("query file out of range")?;
    ensure!(
        source.source.get(start..end).is_some(),
        "selection is not a valid UTF-8 source range"
    );
    let document = Document::new(&source.ast)?;
    let query = document.normalize(document.select_range(start, end)?, view)?;
    let mut result = SearchResult {
        schema: "riffcat-source-search/1".into(),
        query,
        matches: Vec::new(),
        unsupported: Vec::new(),
        compared: 0,
        excluded_self: 0,
        truncated: false,
    };
    let mut visited = 0;
    for artifact in artifacts {
        let doc = Document::new(&artifact.ast)?;
        for (id, node) in doc.candidates() {
            visited += 1;
            if visited > MAX_NODES {
                result.truncated = true;
                return Ok(result);
            }
            if node["nodeType"] != result.query.kind {
                continue;
            }
            let occurrence = span(node)?;
            if artifact
                .source
                .get(
                    occurrence.start
                        ..occurrence
                            .start
                            .checked_add(occurrence.length)
                            .context("span overflow")?,
                )
                .is_none()
            {
                bail!("invalid candidate source span in {}", artifact.path);
            }
            if artifact.source == source.source && occurrence == result.query.span {
                result.excluded_self += 1;
                continue;
            }
            match doc.normalize(id, view) {
                Ok(selection) => {
                    result.compared += 1;
                    if selection.normal == result.query.normal {
                        result.matches.push(SourceMatch {
                            path: artifact.path.clone(),
                            selection,
                        });
                    }
                }
                Err(error) => result
                    .unsupported
                    .push(json!({"path": artifact.path, "id": id, "reason": error.to_string()})),
            }
        }
    }
    result
        .matches
        .sort_by(|a, b| (&a.path, a.selection.span.start).cmp(&(&b.path, b.selection.span.start)));
    Ok(result)
}

pub struct Document<'a> {
    nodes: BTreeMap<i64, &'a Value>,
}

fn children(value: &Value) -> Vec<(&str, usize, &Value)> {
    let mut result = Vec::new();
    if let Some(object) = value.as_object() {
        for (role, child) in object {
            if child.get("nodeType").is_some() {
                result.push((role.as_str(), 0, child));
            } else if let Some(array) = child.as_array() {
                for (index, item) in array.iter().enumerate() {
                    if item.get("nodeType").is_some() {
                        result.push((role.as_str(), index, item));
                    }
                }
            }
        }
    }
    result.sort_by(|a, b| (a.0, a.1).cmp(&(b.0, b.1)));
    result
}

fn collect<'a>(node: &'a Value, nodes: &mut BTreeMap<i64, &'a Value>, depth: usize) -> Result<()> {
    ensure!(
        depth <= 256 && nodes.len() < MAX_NODES,
        "AST budget exceeded"
    );
    let id = node["id"].as_i64().context("AST node missing integer ID")?;
    ensure!(
        nodes.insert(id, node).is_none(),
        "duplicate AST node ID {id}"
    );
    for (_, _, child) in children(node) {
        collect(child, nodes, depth + 1)?;
    }
    Ok(())
}

pub fn span(node: &Value) -> Result<Span> {
    let parts: Vec<_> = node["src"]
        .as_str()
        .context("missing source span")?
        .split(':')
        .collect();
    ensure!(parts.len() == 3, "invalid source span");
    Ok(Span {
        start: parts[0].parse()?,
        length: parts[1].parse()?,
        file: parts[2].parse()?,
    })
}

impl<'a> Document<'a> {
    pub fn new(root: &'a Value) -> Result<Self> {
        let mut nodes = BTreeMap::new();
        collect(root, &mut nodes, 0)?;
        Ok(Self { nodes })
    }

    pub fn candidates(&self) -> impl Iterator<Item = (i64, &'a Value)> + '_ {
        self.nodes.iter().map(|(&id, &node)| (id, node))
    }

    /// Smallest enclosing node. Ambiguous equal spans are resolved by subtree size,
    /// then producer ID for occurrence selection only, never content identity.
    pub fn select_range(&self, start: usize, end: usize) -> Result<i64> {
        ensure!(start < end, "selection must contain at least one byte");
        self.nodes
            .iter()
            .filter_map(|(&id, node)| {
                let s = span(node).ok()?;
                (s.start <= start && s.start.checked_add(s.length)? >= end)
                    .then_some((s.length, id))
            })
            .min()
            .map(|(_, id)| id)
            .context("no AST node encloses selection")
    }

    pub fn normalize(&self, id: i64, view: View) -> Result<Selection> {
        let root = *self.nodes.get(&id).context("unknown selection ID")?;
        let mut ordered = Vec::new();
        fn visit<'a>(node: &'a Value, ordered: &mut Vec<&'a Value>) {
            ordered.push(node);
            for (_, _, child) in children(node) {
                visit(child, ordered);
            }
        }
        visit(root, &mut ordered);
        let positions: BTreeMap<_, _> = ordered
            .iter()
            .enumerate()
            .map(|(index, node)| (node["id"].as_i64().unwrap(), index))
            .collect();
        let mut ports = Vec::new();
        let mut records = Vec::new();
        for node in ordered {
            let kind = node["nodeType"].as_str().context("missing node type")?;
            ensure!(
                crate::spec_for(kind).is_some(),
                "unsupported node type {kind}"
            );
            ensure!(
                kind != "InlineAssembly",
                "inline assembly requires a separate binding contract"
            );
            if view != View::Names && node["names"].as_array().is_some_and(|n| !n.is_empty()) {
                bail!("named arguments unsupported in binding view");
            }
            let mut fields = BTreeMap::new();
            for (key, value) in node.as_object().unwrap() {
                if value.get("nodeType").is_some() {
                    continue;
                }
                if let Some(array) = value.as_array() {
                    if array.iter().any(|n| n.get("nodeType").is_some()) {
                        ensure!(
                            array
                                .iter()
                                .all(|n| n.is_null() || n.get("nodeType").is_some()),
                            "mixed AST array {key}"
                        );
                        // Retain holes and array length in addition to child positions.
                        fields.insert(format!("array:{key}"), json!(array.len()));
                        continue;
                    }
                }
                match key.as_str() {
                    "id"
                    | "src"
                    | "nameLocation"
                    | "nameLocations"
                    | "memberLocation"
                    | "license"
                    | "scope"
                    | "typeDescriptions"
                    | "argumentTypes"
                    | "isConstant"
                    | "isPure"
                    | "isLValue"
                    | "lValueRequested"
                    | "isSimpleCounterLoop"
                    | "commonType"
                    | "functionSelector"
                    | "errorSelector"
                    | "eventSelector"
                    | "canonicalName"
                    | "absolutePath"
                    | "exportedSymbols"
                    | "linearizedBaseContracts"
                    | "contractDependencies"
                    | "usedErrors"
                    | "usedEvents"
                    | "baseFunctions"
                    | "baseModifiers"
                    | "assignments"
                    | "fullyImplemented" => continue,
                    "referencedDeclaration" | "functionReturnParameters" => {
                        if value.is_null() {
                            continue;
                        }
                        let target = value.as_i64().context("noninteger reference")?;
                        let reference = if target < 0 {
                            // Builtin identity retains producer ID and spelling.
                            json!({"builtin": target})
                        } else if let Some(index) = positions.get(&target) {
                            json!({"local": index})
                        } else {
                            let declaration = self
                                .nodes
                                .get(&target)
                                .context("unresolved external declaration")?;
                            let port =
                                ports
                                    .iter()
                                    .position(|&id| id == target)
                                    .unwrap_or_else(|| {
                                        ports.push(target);
                                        ports.len() - 1
                                    });
                            json!({"port": port, "kind": declaration["nodeType"]})
                        };
                        fields.insert(key.clone(), reference);
                    }
                    "overloadedDeclarations" => ensure!(
                        value.as_array().is_some_and(|a| a.is_empty()),
                        "unresolved overloads"
                    ),
                    "name" | "memberName" | "namePath" => {
                        let resolved = node["referencedDeclaration"]
                            .as_i64()
                            .is_some_and(|id| id >= 0);
                        let declaration = matches!(
                            kind,
                            "VariableDeclaration"
                                | "FunctionDefinition"
                                | "ModifierDefinition"
                                | "StructDefinition"
                                | "EnumDefinition"
                                | "EnumValue"
                                | "ContractDefinition"
                                | "EventDefinition"
                                | "ErrorDefinition"
                                | "UserDefinedValueTypeDefinition"
                        );
                        if view == View::Names || !(resolved || declaration) {
                            fields.insert(key.clone(), value.clone());
                        }
                    }
                    "value" | "hexValue"
                        if kind == "Literal" && view == View::BindingsIgnoreLiterals =>
                    {
                        fields.insert(key.clone(), json!("<literal>"));
                    }
                    // These are explicit syntax fields. New producer fields fail
                    // closed until classified as content or occurrence metadata.
                    "nodeType"
                    | "operator"
                    | "prefix"
                    | "kind"
                    | "value"
                    | "hexValue"
                    | "subdenomination"
                    | "visibility"
                    | "stateMutability"
                    | "mutability"
                    | "storageLocation"
                    | "stateVariable"
                    | "constant"
                    | "indexed"
                    | "anonymous"
                    | "virtual"
                    | "abstract"
                    | "contractKind"
                    | "isInlineArray"
                    | "tryCall"
                    | "names"
                    | "literals"
                    | "unitAlias"
                    | "file"
                    | "global"
                    | "body"
                    | "expression"
                    | "initialValue"
                    | "initializationExpression"
                    | "condition"
                    | "loopExpression"
                    | "falseBody"
                    | "trueBody"
                    | "overrides"
                    | "typeName"
                    | "baseType"
                    | "length"
                    | "startExpression"
                    | "endExpression"
                    | "indexExpression"
                    | "modifiers"
                    | "nodes"
                    | "statements"
                    | "members"
                    | "parameters"
                    | "returnParameters"
                    | "baseContracts"
                    | "arguments"
                    | "declarations"
                    | "components"
                    | "options"
                    | "clauses"
                    | "errorName"
                    | "payable"
                    | "implemented"
                    | "keyName"
                    | "valueName" => {
                        fields.insert(key.clone(), value.clone());
                    }
                    other => bail!("unsupported AST field {kind}.{other}"),
                }
            }
            let edges: Vec<_> = children(node).into_iter().map(|(role, index, child)| {
                json!({"role": role, "index": index, "target": positions[&child["id"].as_i64().unwrap()]})
            }).collect();
            records.push(json!({"fields": fields, "children": edges}));
        }
        Ok(Selection {
            id,
            kind: root["nodeType"].as_str().unwrap().into(),
            name: root["name"].as_str().map(str::to_owned),
            span: span(root)?,
            normal: NormalAst {
                contract: CONTRACT.into(),
                view,
                nodes: records,
            },
            external_bindings: ports,
        })
    }
}
