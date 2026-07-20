use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::dimension::Dimension;
use crate::error::CatalogError;
use crate::key::{GraphKey, NodeKey};
use crate::text::Name;
use crate::value::Value;

/// A dimension-tagged (name, value) pair on a node.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Field {
    pub dimension: Dimension,
    pub name: Name,
    pub value: Value,
}

impl Field {
    pub fn new(
        dimension: Dimension,
        name: impl Into<String>,
        value: impl Into<Value>,
    ) -> Result<Self, CatalogError> {
        Ok(Self {
            dimension,
            name: Name::new(name, "field name")?,
            value: value.into(),
        })
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Node {
    pub key: NodeKey,
    pub kind: Name,
    pub fields: Vec<Field>,
}

impl Node {
    pub fn new(key: NodeKey, kind: impl Into<String>) -> Result<Self, CatalogError> {
        Ok(Self {
            key,
            kind: Name::new(kind, "node kind")?,
            fields: Vec::new(),
        })
    }
}

/// An ordered, labeled containment edge — the Merkle skeleton.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChildEdge {
    pub parent: NodeKey,
    pub label: Name,
    pub ordinal: u32,
    pub child: NodeKey,
}

impl ChildEdge {
    pub fn new(
        parent: NodeKey,
        label: impl Into<String>,
        ordinal: u32,
        child: NodeKey,
    ) -> Result<Self, CatalogError> {
        Ok(Self {
            parent,
            label: Name::new(label, "child edge label")?,
            ordinal,
            child,
        })
    }
}

/// Closed set of edge roles — roles define hashing semantics, so they are not
/// open strings. Only `Dependency` participates in cycle/SCC analysis.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EdgeRole {
    Graph,
    Control,
    Data,
    Reference,
    Call,
    Dependency,
    Origin,
}

impl EdgeRole {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Graph => "graph",
            Self::Control => "control",
            Self::Data => "data",
            Self::Reference => "reference",
            Self::Call => "call",
            Self::Dependency => "dependency",
            Self::Origin => "origin",
        }
    }

    /// Whether this role participates in cycle checks and SCC condensation.
    pub const fn is_recursive(self) -> bool {
        matches!(self, Self::Dependency)
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Edge {
    pub source: NodeKey,
    pub label: Name,
    pub target: NodeKey,
    pub role: EdgeRole,
    /// Dimension-tagged payload carried on the edge (mirrors [`Node::fields`]).
    ///
    /// This is provenance-side metadata, e.g. the compiler phase that introduced
    /// an `EdgeRole::Origin` edge. It is inert to every facet address: the
    /// structural fold reads only `role`, `label`, and endpoint digests, and
    /// `EdgeRole::Origin` edges are excluded from the fold entirely. The field is
    /// skip-serialized when empty, so an edge with no payload keeps its frozen
    /// wire form unchanged.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub fields: Vec<Field>,
}

impl Edge {
    pub fn new(
        source: NodeKey,
        label: impl Into<String>,
        target: NodeKey,
        role: EdgeRole,
    ) -> Result<Self, CatalogError> {
        Ok(Self {
            source,
            label: Name::new(label, "edge label")?,
            target,
            role,
            fields: Vec::new(),
        })
    }

    /// An edge carrying dimension-tagged payload fields (see [`Edge::fields`]).
    pub fn with_fields(
        source: NodeKey,
        label: impl Into<String>,
        target: NodeKey,
        role: EdgeRole,
        fields: Vec<Field>,
    ) -> Result<Self, CatalogError> {
        Ok(Self {
            source,
            label: Name::new(label, "edge label")?,
            target,
            role,
            fields,
        })
    }
}

/// The canonical interchange form of one lowered unit.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Graph {
    pub graph_key: GraphKey,
    #[serde(with = "crate::serde_pairs")]
    pub nodes: BTreeMap<NodeKey, Node>,
    pub children: Vec<ChildEdge>,
    pub edges: Vec<Edge>,
}

impl Graph {
    pub fn new(graph_key: GraphKey) -> Self {
        Self {
            graph_key,
            nodes: BTreeMap::new(),
            children: Vec::new(),
            edges: Vec::new(),
        }
    }

    pub fn add_node(&mut self, key: NodeKey, kind: impl Into<String>) -> Result<(), CatalogError> {
        if self.nodes.contains_key(&key) {
            return Err(CatalogError::DuplicateNode {
                key: key.canonical_key(),
            });
        }
        let node = Node::new(key.clone(), kind)?;
        self.nodes.insert(key, node);
        Ok(())
    }

    pub fn add_field(
        &mut self,
        node: &NodeKey,
        dimension: Dimension,
        name: impl Into<String>,
        value: impl Into<Value>,
    ) -> Result<(), CatalogError> {
        let Some(node) = self.nodes.get_mut(node) else {
            return Err(CatalogError::MissingNode {
                key: node.canonical_key(),
            });
        };
        node.fields.push(Field::new(dimension, name, value)?);
        Ok(())
    }

    pub fn add_child(
        &mut self,
        parent: &NodeKey,
        label: impl Into<String>,
        ordinal: u32,
        child: &NodeKey,
    ) -> Result<(), CatalogError> {
        self.require_node(parent)?;
        self.require_node(child)?;
        self.children.push(ChildEdge::new(
            parent.clone(),
            label,
            ordinal,
            child.clone(),
        )?);
        Ok(())
    }

    pub fn add_edge(
        &mut self,
        source: &NodeKey,
        label: impl Into<String>,
        target: &NodeKey,
        role: EdgeRole,
    ) -> Result<(), CatalogError> {
        self.require_node(source)?;
        self.require_node(target)?;
        self.edges
            .push(Edge::new(source.clone(), label, target.clone(), role)?);
        Ok(())
    }

    /// Add an edge carrying dimension-tagged payload fields (see
    /// [`Edge::fields`]). Used to attach provenance (e.g. the introducing phase)
    /// to an `EdgeRole::Origin` edge without perturbing any facet address.
    pub fn add_edge_with_fields(
        &mut self,
        source: &NodeKey,
        label: impl Into<String>,
        target: &NodeKey,
        role: EdgeRole,
        fields: Vec<Field>,
    ) -> Result<(), CatalogError> {
        self.require_node(source)?;
        self.require_node(target)?;
        self.edges.push(Edge::with_fields(
            source.clone(),
            label,
            target.clone(),
            role,
            fields,
        )?);
        Ok(())
    }

    pub fn validate(&self) -> Result<(), CatalogError> {
        for (key, node) in &self.nodes {
            if key != &node.key {
                return Err(CatalogError::NodeKeyMismatch {
                    map_key: key.canonical_key(),
                    node_key: node.key.canonical_key(),
                });
            }
        }
        for child in &self.children {
            self.require_node(&child.parent)?;
            self.require_node(&child.child)?;
        }
        for edge in &self.edges {
            self.require_node(&edge.source)?;
            self.require_node(&edge.target)?;
        }
        Ok(())
    }

    fn require_node(&self, key: &NodeKey) -> Result<(), CatalogError> {
        if self.nodes.contains_key(key) {
            Ok(())
        } else {
            Err(CatalogError::MissingNode {
                key: key.canonical_key(),
            })
        }
    }
}

/// Producer-facing sink so lowerings can be written against a trait.
pub trait GraphSink {
    fn add_node(&mut self, key: NodeKey, kind: impl Into<String>) -> Result<(), CatalogError>;
    fn add_field(
        &mut self,
        node: &NodeKey,
        dimension: Dimension,
        name: impl Into<String>,
        value: impl Into<Value>,
    ) -> Result<(), CatalogError>;
    fn add_child(
        &mut self,
        parent: &NodeKey,
        label: impl Into<String>,
        ordinal: u32,
        child: &NodeKey,
    ) -> Result<(), CatalogError>;
    fn add_edge(
        &mut self,
        source: &NodeKey,
        label: impl Into<String>,
        target: &NodeKey,
        role: EdgeRole,
    ) -> Result<(), CatalogError>;
}

impl GraphSink for Graph {
    fn add_node(&mut self, key: NodeKey, kind: impl Into<String>) -> Result<(), CatalogError> {
        Graph::add_node(self, key, kind)
    }

    fn add_field(
        &mut self,
        node: &NodeKey,
        dimension: Dimension,
        name: impl Into<String>,
        value: impl Into<Value>,
    ) -> Result<(), CatalogError> {
        Graph::add_field(self, node, dimension, name, value)
    }

    fn add_child(
        &mut self,
        parent: &NodeKey,
        label: impl Into<String>,
        ordinal: u32,
        child: &NodeKey,
    ) -> Result<(), CatalogError> {
        Graph::add_child(self, parent, label, ordinal, child)
    }

    fn add_edge(
        &mut self,
        source: &NodeKey,
        label: impl Into<String>,
        target: &NodeKey,
        role: EdgeRole,
    ) -> Result<(), CatalogError> {
        Graph::add_edge(self, source, label, target, role)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::key::EntityKey;

    fn key(kind: &str, local: &str) -> NodeKey {
        NodeKey::entity(EntityKey::new(kind, "demo", local).unwrap())
    }

    fn graph() -> Graph {
        Graph::new(
            GraphKey::new(
                EntityKey::new("test.body", "demo", "body:0").unwrap(),
                "body",
            )
            .unwrap(),
        )
    }

    #[test]
    fn rejects_duplicate_nodes_and_missing_endpoints() {
        let mut g = graph();
        let a = key("test.node", "a");
        let b = key("test.node", "b");
        g.add_node(a.clone(), "node").unwrap();
        assert!(matches!(
            g.add_node(a.clone(), "node"),
            Err(CatalogError::DuplicateNode { .. })
        ));
        assert!(matches!(
            g.add_child(&a, "x", 0, &b),
            Err(CatalogError::MissingNode { .. })
        ));
        assert!(matches!(
            g.add_edge(&a, "x", &b, EdgeRole::Reference),
            Err(CatalogError::MissingNode { .. })
        ));
    }

    #[test]
    fn accepts_fields_children_and_edges() {
        let mut g = graph();
        let a = key("test.node", "a");
        let b = key("test.node", "b");
        g.add_node(a.clone(), "node").unwrap();
        g.add_node(b.clone(), "literal").unwrap();
        g.add_field(&b, Dimension::Constants, "value", 1u64)
            .unwrap();
        g.add_child(&a, "expr", 0, &b).unwrap();
        g.add_edge(&b, "uses", &a, EdgeRole::Reference).unwrap();
        g.validate().unwrap();
    }

    #[test]
    fn serde_round_trip() {
        let mut g = graph();
        let a = key("test.node", "a");
        g.add_node(a.clone(), "node").unwrap();
        g.add_field(&a, Dimension::Names, "name", "x").unwrap();
        let json = serde_json::to_string(&g).unwrap();
        let back: Graph = serde_json::from_str(&json).unwrap();
        assert_eq!(back, g);
    }
}
