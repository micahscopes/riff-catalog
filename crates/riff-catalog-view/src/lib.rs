//! Declarative graph views.
//!
//! A view plan selects roots, computes a forward reachability closure, and
//! projects the resulting graph onto a dimension set. The plan has a canonical
//! digest so its semantics can participate in the output hash policy.

use std::cmp::Ordering;
use std::collections::{BTreeMap, BTreeSet, VecDeque};

use riff_catalog_core::{Digest, Dimension, EdgeRole, Field, Graph, NodeKey};
use thiserror::Error;

pub const VIEW_LANGUAGE: &str = "riffcat-view/1";
pub const VIEW_SCHEMA_VERSION: u32 = 1;

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum RootSelector {
    ChildParent(String),
    ChildTarget(String),
    NodeKind(String),
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum ChildTraversal {
    #[default]
    None,
    Labels(BTreeSet<String>),
    All,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ViewPlan {
    pub schema_version: u32,
    pub name: String,
    pub input_level: String,
    pub roots: BTreeSet<RootSelector>,
    pub children: ChildTraversal,
    pub edge_roles: BTreeSet<EdgeRole>,
    pub dimensions: BTreeSet<Dimension>,
}

#[derive(Clone, Debug, Error, PartialEq, Eq)]
pub enum ViewError {
    #[error("line {line}: {message}")]
    Parse { line: usize, message: String },
    #[error("view matched no root nodes")]
    NoRoots,
    #[error(transparent)]
    Graph(#[from] riff_catalog_core::CatalogError),
}

impl ViewPlan {
    pub fn parse(source: &str) -> Result<Self, ViewError> {
        let mut language = None;
        let mut name = None;
        let mut input_level = None;
        let mut roots = BTreeSet::new();
        let mut children = ChildTraversal::None;
        let mut edge_roles = BTreeSet::new();
        let mut dimensions = BTreeSet::new();

        for (index, raw) in source.lines().enumerate() {
            let line_number = index + 1;
            let line = raw.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }

            let (directive, rest) = split_directive(line);
            match directive {
                "language" => set_once(
                    &mut language,
                    quoted(rest, line_number, "language")?,
                    line_number,
                    "language",
                )?,
                "view" => set_once(
                    &mut name,
                    quoted(rest, line_number, "view")?,
                    line_number,
                    "view",
                )?,
                "input" => set_once(
                    &mut input_level,
                    quoted(rest, line_number, "input")?,
                    line_number,
                    "input",
                )?,
                "root" => parse_root(rest, line_number, &mut roots)?,
                "traverse" => parse_traversal(rest, line_number, &mut children, &mut edge_roles)?,
                "retain" => {
                    for item in csv(rest, line_number, "dimension")? {
                        let dimension = Dimension::parse(&item).ok_or_else(|| {
                            parse_error(line_number, format!("unknown dimension `{item}`"))
                        })?;
                        dimensions.insert(dimension);
                    }
                }
                other => {
                    return Err(parse_error(
                        line_number,
                        format!("unknown directive `{other}`"),
                    ));
                }
            }
        }

        let language = required(language, "language")?;
        if language != VIEW_LANGUAGE {
            return Err(parse_error(
                1,
                format!("unsupported language `{language}`; expected `{VIEW_LANGUAGE}`"),
            ));
        }
        let name = required(name, "view")?;
        let input_level = required(input_level, "input")?;
        if roots.is_empty() {
            return Err(parse_error(1, "view must define at least one root"));
        }
        if dimensions.is_empty() {
            return Err(parse_error(1, "view must retain at least one dimension"));
        }

        Ok(Self {
            schema_version: VIEW_SCHEMA_VERSION,
            name,
            input_level,
            roots,
            children,
            edge_roles,
            dimensions,
        })
    }

    /// A formatting-independent digest of the view semantics.
    pub fn plan_id(&self) -> Digest {
        let mut bytes = Vec::new();
        push_str(&mut bytes, "riffcat.view-plan");
        push_u32(&mut bytes, self.schema_version);
        push_str(&mut bytes, &self.name);
        push_str(&mut bytes, &self.input_level);

        push_u32(&mut bytes, self.roots.len() as u32);
        for root in &self.roots {
            match root {
                RootSelector::ChildParent(label) => {
                    push_str(&mut bytes, "child-parent");
                    push_str(&mut bytes, label);
                }
                RootSelector::ChildTarget(label) => {
                    push_str(&mut bytes, "child-target");
                    push_str(&mut bytes, label);
                }
                RootSelector::NodeKind(kind) => {
                    push_str(&mut bytes, "node-kind");
                    push_str(&mut bytes, kind);
                }
            }
        }

        match &self.children {
            ChildTraversal::None => push_str(&mut bytes, "children-none"),
            ChildTraversal::All => push_str(&mut bytes, "children-all"),
            ChildTraversal::Labels(labels) => {
                push_str(&mut bytes, "children-labels");
                push_u32(&mut bytes, labels.len() as u32);
                for label in labels {
                    push_str(&mut bytes, label);
                }
            }
        }

        push_u32(&mut bytes, self.edge_roles.len() as u32);
        for role in &self.edge_roles {
            push_str(&mut bytes, role.as_str());
        }
        push_u32(&mut bytes, self.dimensions.len() as u32);
        for dimension in &self.dimensions {
            push_str(&mut bytes, dimension.as_str());
        }

        Digest::from_bytes(*blake3::hash(&bytes).as_bytes())
    }

    /// Hash-policy level for materialized output. Including the full plan id
    /// prevents a changed plan from retaining an old view identity.
    pub fn output_level(&self) -> String {
        format!("view:{}@{}", self.name, self.plan_id())
    }

    pub fn materialize(&self, input: &Graph) -> Result<Graph, ViewError> {
        input.validate()?;

        let mut live = BTreeSet::new();
        for root in &self.roots {
            match root {
                RootSelector::ChildParent(label) => {
                    live.extend(
                        input
                            .children
                            .iter()
                            .filter(|child| child.label.as_str() == label)
                            .map(|child| child.parent.clone()),
                    );
                }
                RootSelector::ChildTarget(label) => {
                    live.extend(
                        input
                            .children
                            .iter()
                            .filter(|child| child.label.as_str() == label)
                            .map(|child| child.child.clone()),
                    );
                }
                RootSelector::NodeKind(kind) => {
                    live.extend(
                        input
                            .nodes
                            .iter()
                            .filter(|(_, node)| node.kind.as_str() == kind)
                            .map(|(key, _)| key.clone()),
                    );
                }
            }
        }
        if live.is_empty() {
            return Err(ViewError::NoRoots);
        }

        let mut outgoing: BTreeMap<NodeKey, Vec<NodeKey>> = BTreeMap::new();
        for child in &input.children {
            let follows = match &self.children {
                ChildTraversal::None => false,
                ChildTraversal::All => true,
                ChildTraversal::Labels(labels) => labels.contains(child.label.as_str()),
            };
            if follows {
                outgoing
                    .entry(child.parent.clone())
                    .or_default()
                    .push(child.child.clone());
            }
        }
        for edge in &input.edges {
            if self.edge_roles.contains(&edge.role) {
                outgoing
                    .entry(edge.source.clone())
                    .or_default()
                    .push(edge.target.clone());
            }
        }

        let mut queue: VecDeque<NodeKey> = live.iter().cloned().collect();
        while let Some(source) = queue.pop_front() {
            for target in outgoing.get(&source).into_iter().flatten() {
                if live.insert(target.clone()) {
                    queue.push_back(target.clone());
                }
            }
        }

        let mut output = Graph::new(input.graph_key.clone());
        for (key, node) in &input.nodes {
            if !live.contains(key) {
                continue;
            }
            let mut node = node.clone();
            node.fields
                .retain(|field| self.dimensions.contains(&field.dimension));
            node.fields.sort_by(compare_fields);
            output.nodes.insert(key.clone(), node);
        }

        output.children = input
            .children
            .iter()
            .filter(|child| live.contains(&child.parent) && live.contains(&child.child))
            .cloned()
            .collect();
        output.children.sort_by(|left, right| {
            left.parent
                .cmp(&right.parent)
                .then_with(|| left.label.cmp(&right.label))
                .then_with(|| left.ordinal.cmp(&right.ordinal))
                .then_with(|| left.child.cmp(&right.child))
        });

        output.edges = input
            .edges
            .iter()
            .filter(|edge| live.contains(&edge.source) && live.contains(&edge.target))
            .cloned()
            .map(|mut edge| {
                edge.fields
                    .retain(|field| self.dimensions.contains(&field.dimension));
                edge.fields.sort_by(compare_fields);
                edge
            })
            .collect();
        output.edges.sort_by(|left, right| {
            left.source
                .cmp(&right.source)
                .then_with(|| left.role.cmp(&right.role))
                .then_with(|| left.label.cmp(&right.label))
                .then_with(|| left.target.cmp(&right.target))
                .then_with(|| compare_field_slices(&left.fields, &right.fields))
        });

        output.validate()?;
        Ok(output)
    }
}

fn split_directive(line: &str) -> (&str, &str) {
    line.split_once(char::is_whitespace)
        .map_or((line, ""), |(directive, rest)| (directive, rest.trim()))
}

fn set_once(
    slot: &mut Option<String>,
    value: String,
    line: usize,
    directive: &str,
) -> Result<(), ViewError> {
    if slot.replace(value).is_some() {
        return Err(parse_error(
            line,
            format!("duplicate `{directive}` directive"),
        ));
    }
    Ok(())
}

fn required(value: Option<String>, directive: &str) -> Result<String, ViewError> {
    value.ok_or_else(|| parse_error(1, format!("missing `{directive}` directive")))
}

fn parse_root(
    rest: &str,
    line: usize,
    roots: &mut BTreeSet<RootSelector>,
) -> Result<(), ViewError> {
    let (kind, argument) = split_directive(rest);
    let value = quoted(argument, line, "root selector")?;
    let selector = match kind {
        "child-parent" => RootSelector::ChildParent(value),
        "child-target" => RootSelector::ChildTarget(value),
        "node-kind" => RootSelector::NodeKind(value),
        _ => {
            return Err(parse_error(line, format!("unknown root selector `{kind}`")));
        }
    };
    roots.insert(selector);
    Ok(())
}

fn parse_traversal(
    rest: &str,
    line: usize,
    children: &mut ChildTraversal,
    edge_roles: &mut BTreeSet<EdgeRole>,
) -> Result<(), ViewError> {
    let (kind, arguments) = split_directive(rest);
    match kind {
        "children" if arguments.is_empty() => *children = ChildTraversal::All,
        "children" => {
            let labels = csv(arguments, line, "child label")?;
            if !matches!(children, ChildTraversal::All) {
                let retained = match children {
                    ChildTraversal::Labels(retained) => retained,
                    ChildTraversal::None => {
                        *children = ChildTraversal::Labels(BTreeSet::new());
                        let ChildTraversal::Labels(retained) = children else {
                            unreachable!()
                        };
                        retained
                    }
                    ChildTraversal::All => unreachable!(),
                };
                retained.extend(labels);
            }
        }
        "edges" => {
            for item in csv(arguments, line, "edge role")? {
                edge_roles.insert(
                    parse_edge_role(&item)
                        .ok_or_else(|| parse_error(line, format!("unknown edge role `{item}`")))?,
                );
            }
        }
        _ => {
            return Err(parse_error(line, format!("unknown traversal `{kind}`")));
        }
    }
    Ok(())
}

fn parse_edge_role(value: &str) -> Option<EdgeRole> {
    match value {
        "graph" => Some(EdgeRole::Graph),
        "control" => Some(EdgeRole::Control),
        "data" => Some(EdgeRole::Data),
        "reference" => Some(EdgeRole::Reference),
        "call" => Some(EdgeRole::Call),
        "dependency" => Some(EdgeRole::Dependency),
        "origin" => Some(EdgeRole::Origin),
        _ => None,
    }
}

fn quoted(rest: &str, line: usize, what: &str) -> Result<String, ViewError> {
    let Some(inner) = rest
        .strip_prefix('"')
        .and_then(|text| text.strip_suffix('"'))
    else {
        return Err(parse_error(line, format!("{what} must be quoted")));
    };
    if inner.is_empty() || inner.contains('"') {
        return Err(parse_error(line, format!("invalid {what}")));
    }
    Ok(inner.to_string())
}

fn csv(rest: &str, line: usize, what: &str) -> Result<Vec<String>, ViewError> {
    let values: Vec<String> = rest
        .split(',')
        .map(str::trim)
        .filter(|item| !item.is_empty())
        .map(str::to_string)
        .collect();
    if values.is_empty() {
        return Err(parse_error(line, format!("expected at least one {what}")));
    }
    Ok(values)
}

fn parse_error(line: usize, message: impl Into<String>) -> ViewError {
    ViewError::Parse {
        line,
        message: message.into(),
    }
}

fn push_u32(bytes: &mut Vec<u8>, value: u32) {
    bytes.extend_from_slice(&value.to_le_bytes());
}

fn push_str(bytes: &mut Vec<u8>, value: &str) {
    bytes.extend_from_slice(&(value.len() as u64).to_le_bytes());
    bytes.extend_from_slice(value.as_bytes());
}

fn compare_fields(left: &Field, right: &Field) -> Ordering {
    left.dimension
        .cmp(&right.dimension)
        .then_with(|| left.name.cmp(&right.name))
        .then_with(|| left.value.cmp(&right.value))
}

fn compare_field_slices(left: &[Field], right: &[Field]) -> Ordering {
    left.iter()
        .zip(right)
        .map(|(left, right)| compare_fields(left, right))
        .find(|ordering| !ordering.is_eq())
        .unwrap_or_else(|| left.len().cmp(&right.len()))
}

#[cfg(test)]
mod tests {
    use riff_catalog_core::{
        CyclePolicy, DigestRequest, EntityKey, Facet, GraphKey, HashPolicy, ViewMode, digest_graph,
    };

    use super::*;

    fn parse(source: &str) -> ViewPlan {
        ViewPlan::parse(source).unwrap()
    }

    fn yul_view(order: bool) -> ViewPlan {
        if order {
            parse(
                r#"
language "riffcat-view/1"
view "yul.reachable/1"
input "yul-ssa-cfg/1"
root child-parent "entry"
traverse children entry, arg, insn, in, imm, phi-in, cond, ret
traverse edges dependency, data, control
retain structure, types, constants
"#,
            )
        } else {
            parse(
                r#"
retain constants, structure, types
traverse edges control, dependency, data
root child-parent "entry"
input "yul-ssa-cfg/1"
traverse children ret, cond, phi-in, imm, in, insn, arg, entry
view "yul.reachable/1"
language "riffcat-view/1"
"#,
            )
        }
    }

    fn node(owner: &str, kind: &str, local: &str) -> NodeKey {
        NodeKey::entity(EntityKey::new(kind, owner, local).unwrap())
    }

    fn sample_graph(reverse_storage: bool) -> Graph {
        let owner = "yulssa:test";
        let key = GraphKey::new(EntityKey::new("yulssa.fn", owner, "fn:f").unwrap(), "fn").unwrap();
        let mut graph = Graph::new(key);
        let function = node(owner, "yulssa.fn", "fn:f");
        let entry = node(owner, "yulssa.block", "fn:f/b:entry");
        let next = node(owner, "yulssa.block", "fn:f/b:next");
        let dead = node(owner, "yulssa.block", "fn:f/b:dead");
        let entry_insn = node(owner, "yulssa.insn", "fn:f/b:entry/i:0");
        let next_insn = node(owner, "yulssa.insn", "fn:f/b:next/i:0");
        let dead_insn = node(owner, "yulssa.insn", "fn:f/b:dead/i:0");

        for (key, kind) in [
            (&function, "yulssa.fn"),
            (&entry, "yulssa.block"),
            (&next, "yulssa.block"),
            (&dead, "yulssa.block"),
            (&entry_insn, "yulssa.insn"),
            (&next_insn, "yulssa.insn"),
            (&dead_insn, "yulssa.insn"),
        ] {
            graph.add_node(key.clone(), kind).unwrap();
        }
        graph
            .add_field(&entry_insn, Dimension::Structure, "op", "mstore")
            .unwrap();
        graph
            .add_field(&entry_insn, Dimension::Names, "debug_name", "temporary")
            .unwrap();
        graph.add_child(&function, "entry", 0, &entry).unwrap();
        graph.add_child(&function, "block", 0, &next).unwrap();
        graph.add_child(&function, "block", 0, &dead).unwrap();
        graph.add_child(&entry, "insn", 0, &entry_insn).unwrap();
        graph.add_child(&next, "insn", 0, &next_insn).unwrap();
        graph.add_child(&dead, "insn", 0, &dead_insn).unwrap();
        graph
            .add_edge(&entry, "jump:0", &next, EdgeRole::Dependency)
            .unwrap();
        graph
            .add_edge(&next_insn, "def", &entry_insn, EdgeRole::Data)
            .unwrap();

        if reverse_storage {
            graph.children.reverse();
            graph.edges.reverse();
            for node in graph.nodes.values_mut() {
                node.fields.reverse();
            }
        }
        graph
    }

    #[test]
    fn plan_id_ignores_formatting_and_directive_order() {
        let left = yul_view(true);
        let right = yul_view(false);
        assert_eq!(left, right);
        assert_eq!(left.plan_id(), right.plan_id());
    }

    #[test]
    fn reachable_view_excludes_dead_blocks_and_unretained_fields() {
        let plan = yul_view(true);
        let graph = sample_graph(false);
        let output = plan.materialize(&graph).unwrap();

        assert_eq!(output.nodes.len(), 5);
        assert!(
            output
                .nodes
                .keys()
                .all(|key| !key.canonical_key().contains("b:dead"))
        );
        assert!(output.nodes.values().all(|node| {
            node.fields
                .iter()
                .all(|field| field.dimension != Dimension::Names)
        }));
    }

    #[test]
    fn reordered_input_materializes_and_hashes_identically() {
        let plan = yul_view(true);
        let left = plan.materialize(&sample_graph(false)).unwrap();
        let right = plan.materialize(&sample_graph(true)).unwrap();
        assert_eq!(left, right);

        let policy = HashPolicy::new(
            plan.output_level(),
            ViewMode::AnonymousShape,
            CyclePolicy::CondenseScc,
        )
        .unwrap();
        let digest = |graph: &Graph| {
            let request = DigestRequest::new(
                graph.graph_key.clone(),
                policy.clone(),
                plan.dimensions.clone(),
            )
            .unwrap();
            let result = digest_graph(&request, graph).unwrap();
            result
                .hashes
                .facet_address(&Facet::new(policy.policy_id(), plan.dimensions.clone()).unwrap())
                .unwrap()
                .address_digest()
        };
        assert_eq!(digest(&left), digest(&right));
    }

    #[test]
    fn rejects_unknown_language_and_empty_roots() {
        let unknown = ViewPlan::parse(
            r#"
language "other/1"
view "x/1"
input "x/1"
root node-kind "x"
retain structure
"#,
        )
        .unwrap_err();
        assert!(unknown.to_string().contains("unsupported language"));

        let empty = ViewPlan::parse(
            r#"
language "riffcat-view/1"
view "x/1"
input "x/1"
retain structure
"#,
        )
        .unwrap_err();
        assert!(empty.to_string().contains("at least one root"));
    }
}
