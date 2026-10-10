//! Recipe declarations: what a recipe's graphs may contain, written as data.
//!
//! A recipe is a function from an artifact to graphs. Its declaration says
//! which node kinds occur, which facts each kind carries and in which dimension,
//! how children hang under their parents, which edges connect what, and whether
//! the graphs are acyclic. Every reader uses the same value:
//!
//! - ingest checks every graph a producer emits ([`RecipeDecl::check`]);
//! - the dimensions a recipe fills are read off it ([`RecipeDecl::fills`]);
//! - facet and transform field names are checked against it
//!   ([`RecipeDecl::field_dimensions`]);
//! - two producers of one recipe (two compilers, a compiler and a model) are
//!   tested against the same declaration.
//!
//! Origin edges are provenance, never hashed, and always allowed.

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};

use crate::dimension::Dimension;
use crate::graph::{EdgeRole, Graph};
use crate::text::Name;
use crate::value::Value;

/// The type of a field's value.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ValueType {
    Text,
    Bool,
    U64,
    I64,
    Bytes,
}

impl ValueType {
    pub fn of(value: &Value) -> Self {
        match value {
            Value::Text(_) => Self::Text,
            Value::Bool(_) => Self::Bool,
            Value::U64(_) => Self::U64,
            Value::I64(_) => Self::I64,
            Value::Bytes(_) => Self::Bytes,
        }
    }
}

/// How many times a field occurs on one node.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Occurs {
    /// Exactly once.
    One,
    /// At most once.
    Optional,
    /// Any number of times (a counted multiset).
    Many,
}

/// One field a node kind may carry. `dimensions` usually holds one dimension;
/// it holds several when the recipe puts the same field in different bins by
/// position (Fe's query records put `name` in structure, names or types
/// depending on the subtree).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct FieldDecl {
    pub name: Name,
    pub dimensions: BTreeSet<Dimension>,
    pub value: ValueType,
    pub occurs: Occurs,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct KindDecl {
    pub kind: Name,
    pub fields: Vec<FieldDecl>,
}

/// How the children under one label are arranged.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Shape {
    /// An ordered sequence: ordinals 0, 1, ..., n-1, each once.
    Seq,
    /// An unordered collection: every ordinal is 0.
    Set,
    /// One child, ordinal 0. Every parent has it unless the group is
    /// declared optional, in which case a parent has it or has none.
    One,
}

/// Children with `label` under a node of one of `parents` are of one of
/// `children` and arranged as `shape`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChildDecl {
    pub parents: BTreeSet<Name>,
    pub label: Name,
    pub children: BTreeSet<Name>,
    pub shape: Shape,
    /// For [`Shape::One`]: whether a parent may have no child under this
    /// label. Sequences and sets may always be empty.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub optional: bool,
}

/// A label a declaration matches: one exact label, or every label starting
/// with a prefix (written `jump:*`).
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LabelPattern {
    Exact(Name),
    Prefix(Name),
}

impl LabelPattern {
    pub fn matches(&self, label: &str) -> bool {
        match self {
            Self::Exact(name) => name.as_str() == label,
            Self::Prefix(prefix) => label.starts_with(prefix.as_str()),
        }
    }
}

/// Edges with a matching label and `role` go from one of `sources` to one of
/// `targets`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct EdgeDecl {
    pub sources: BTreeSet<Name>,
    pub label: LabelPattern,
    pub targets: BTreeSet<Name>,
    pub role: EdgeRole,
}

/// Which edges a recipe promises never form a cycle.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Acyclic {
    /// Cycles are allowed (control flow, mutual recursion).
    No,
    /// Child edges form no cycle.
    Children,
    /// Child edges and Dependency edges together form no cycle.
    ChildrenAndDependency,
}

/// Everything a recipe's graphs may contain.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RecipeDecl {
    pub recipe: Name,
    /// Kinds a root (a node that is nobody's child) may have.
    pub roots: BTreeSet<Name>,
    /// Whether every graph has exactly one root.
    pub single_root: bool,
    pub kinds: Vec<KindDecl>,
    pub children: Vec<ChildDecl>,
    pub edges: Vec<EdgeDecl>,
    pub acyclic: Acyclic,
    /// How node keys are formed, in words. Keys must not name the world (the
    /// compiler, commit or model that produced the graph), or bound-keys
    /// comparisons across worlds can never agree.
    pub keys: String,
}

/// One way a graph breaks its recipe's declaration.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Violation {
    /// The node (canonical key) the problem is at, when there is one.
    pub at: Option<String>,
    pub problem: String,
}

impl RecipeDecl {
    /// Read a declaration from JSON and check that it is well formed.
    pub fn from_json(text: &str) -> Result<Self, String> {
        let decl: Self = serde_json::from_str(text).map_err(|error| error.to_string())?;
        decl.validate().map_err(|problems| problems.join("; "))?;
        Ok(decl)
    }

    /// Whether the declaration itself is unambiguous: no kind, field or child
    /// group declared twice, and every kind it mentions declared. Lookups take
    /// the first match, so a duplicate would make the meaning depend on order.
    pub fn validate(&self) -> Result<(), Vec<String>> {
        let mut problems = Vec::new();
        let mut kinds = BTreeSet::new();
        for kind in &self.kinds {
            if !kinds.insert(kind.kind.as_str()) {
                problems.push(format!("kind `{}` is declared twice", kind.kind.as_str()));
            }
            let mut fields = BTreeSet::new();
            for field in &kind.fields {
                if !fields.insert(field.name.as_str()) {
                    problems.push(format!("field `{}.{}` is declared twice", kind.kind.as_str(), field.name.as_str()));
                }
                if field.dimensions.is_empty() {
                    problems.push(format!("field `{}.{}` has no dimension", kind.kind.as_str(), field.name.as_str()));
                }
            }
        }
        let known = |what: &str, names: &BTreeSet<Name>, problems: &mut Vec<String>| {
            for name in names {
                if !kinds.contains(name.as_str()) {
                    problems.push(format!("{what} mentions undeclared kind `{}`", name.as_str()));
                }
            }
        };
        known("roots", &self.roots, &mut problems);
        let mut groups = BTreeSet::new();
        for child in &self.children {
            known(&format!("child group `{}`", child.label.as_str()), &child.parents, &mut problems);
            known(&format!("child group `{}`", child.label.as_str()), &child.children, &mut problems);
            for parent in &child.parents {
                if !groups.insert((parent.as_str(), child.label.as_str())) {
                    problems.push(format!(
                        "children `{}` -{}-> are declared twice",
                        parent.as_str(),
                        child.label.as_str()
                    ));
                }
            }
        }
        for edge in &self.edges {
            known("an edge", &edge.sources, &mut problems);
            known("an edge", &edge.targets, &mut problems);
        }
        if problems.is_empty() { Ok(()) } else { Err(problems) }
    }

    fn kind(&self, kind: &str) -> Option<&KindDecl> {
        self.kinds.iter().find(|decl| decl.kind.as_str() == kind)
    }

    /// The dimensions a field may be in, or `None` when the kind does not
    /// declare the field. Facet and transform field names are checked with it.
    pub fn field_dimensions(&self, kind: &str, field: &str) -> Option<&BTreeSet<Dimension>> {
        self.kind(kind)?
            .fields
            .iter()
            .find(|decl| decl.name.as_str() == field)
            .map(|decl| &decl.dimensions)
    }

    /// Whether any kind declares a field with this name in this dimension.
    pub fn declares_field(&self, dimension: Dimension, field: &str) -> bool {
        self.kinds.iter().any(|kind| {
            kind.fields
                .iter()
                .any(|decl| decl.name.as_str() == field && decl.dimensions.contains(&dimension))
        })
    }

    /// The dimensions this recipe puts facts in. Structure is always filled
    /// (kinds, child labels and ordinals). A dimension the recipe never fills
    /// has no meaning at this recipe, so comparing on it is refused.
    pub fn fills(&self) -> BTreeSet<Dimension> {
        let mut out = BTreeSet::from([Dimension::Structure]);
        for kind in &self.kinds {
            for field in &kind.fields {
                out.extend(field.dimensions.iter().copied());
            }
        }
        out
    }

    /// Check a graph against the declaration: kinds, fields (name, dimension,
    /// value type, count), roots, child arrangement and edges. Acyclicity is
    /// checked by the engine (`riff_catalog_core::check_acyclic`), which owns
    /// graph traversal.
    pub fn check(&self, graph: &Graph) -> Result<(), Vec<Violation>> {
        let mut violations = Vec::new();
        let mut bad = |at: Option<String>, problem: String| violations.push(Violation { at, problem });

        for node in graph.nodes.values() {
            let at = Some(node.key.canonical_key());
            let Some(kind) = self.kind(node.kind.as_str()) else {
                bad(at, format!("kind `{}` is not declared", node.kind.as_str()));
                continue;
            };
            let mut counts: BTreeMap<&str, usize> = BTreeMap::new();
            for field in &node.fields {
                let name = field.name.as_str();
                *counts.entry(name).or_default() += 1;
                match kind.fields.iter().find(|decl| decl.name.as_str() == name) {
                    None => bad(at.clone(), format!("field `{name}` is not declared on `{}`", kind.kind.as_str())),
                    Some(decl) => {
                        if !decl.dimensions.contains(&field.dimension) {
                            bad(at.clone(), format!("field `{name}` is in {}, not a declared dimension", field.dimension.as_str()));
                        }
                        if ValueType::of(&field.value) != decl.value {
                            bad(at.clone(), format!("field `{name}` holds {:?}, declared {:?}", ValueType::of(&field.value), decl.value));
                        }
                    }
                }
            }
            for decl in &kind.fields {
                let n = counts.get(decl.name.as_str()).copied().unwrap_or(0);
                let ok = match decl.occurs {
                    Occurs::One => n == 1,
                    Occurs::Optional => n <= 1,
                    Occurs::Many => true,
                };
                if !ok {
                    bad(at.clone(), format!("field `{}` occurs {n} times, declared {:?}", decl.name.as_str(), decl.occurs));
                }
            }
        }

        let kind_of = |key: &crate::key::NodeKey| graph.nodes.get(key).map(|node| node.kind.as_str());
        let mut is_child = BTreeSet::new();
        // Ordinals per (parent, label), keyed by the parent's key itself: one
        // pass over the children, and lookups by key after that.
        let mut groups: BTreeMap<(&crate::key::NodeKey, &str), Vec<u32>> = BTreeMap::new();
        for child in &graph.children {
            is_child.insert(&child.child);
            let (Some(parent_kind), Some(child_kind)) = (kind_of(&child.parent), kind_of(&child.child)) else {
                continue;
            };
            let label = child.label.as_str();
            match self.child_decl(parent_kind, label) {
                None => bad(
                    Some(child.parent.canonical_key()),
                    format!("`{parent_kind}` may not have children labelled `{label}`"),
                ),
                Some(decl) => {
                    if !decl.children.iter().any(|c| c.as_str() == child_kind) {
                        bad(
                            Some(child.parent.canonical_key()),
                            format!("child `{child_kind}` under `{parent_kind}` -{label}-> is not declared"),
                        );
                    }
                    groups.entry((&child.parent, label)).or_default().push(child.ordinal);
                }
            }
        }
        for ((parent, label), ordinals) in &mut groups {
            let Some(decl) = kind_of(parent).and_then(|kind| self.child_decl(kind, label)) else {
                continue;
            };
            ordinals.sort_unstable();
            let ok = match decl.shape {
                Shape::Seq => ordinals.iter().enumerate().all(|(i, &o)| o as usize == i),
                Shape::Set => ordinals.iter().all(|&o| o == 0),
                Shape::One => ordinals[..] == [0],
            };
            if !ok {
                bad(
                    Some(parent.canonical_key()),
                    format!("children -{label}-> have ordinals {ordinals:?}, declared {:?}", decl.shape),
                );
            }
        }

        // A required single child must be present on every parent of its kinds.
        for decl in self.children.iter().filter(|decl| decl.shape == Shape::One && !decl.optional) {
            for node in graph.nodes.values() {
                if !decl.parents.iter().any(|p| p.as_str() == node.kind.as_str()) {
                    continue;
                }
                if !groups.contains_key(&(&node.key, decl.label.as_str())) {
                    bad(
                        Some(node.key.canonical_key()),
                        format!("`{}` has no child -{}->, declared required", node.kind.as_str(), decl.label.as_str()),
                    );
                }
            }
        }

        let roots: Vec<_> = graph.nodes.values().filter(|node| !is_child.contains(&node.key)).collect();
        if self.single_root && roots.len() != 1 {
            bad(None, format!("{} roots, declared exactly one", roots.len()));
        }
        for root in roots {
            if !self.roots.iter().any(|kind| kind.as_str() == root.kind.as_str()) {
                bad(Some(root.key.canonical_key()), format!("`{}` may not be a root", root.kind.as_str()));
            }
        }

        for edge in &graph.edges {
            if edge.role == EdgeRole::Origin {
                continue;
            }
            let (Some(source_kind), Some(target_kind)) = (kind_of(&edge.source), kind_of(&edge.target)) else {
                continue;
            };
            let declared = self.edges.iter().any(|decl| {
                decl.role == edge.role
                    && decl.label.matches(edge.label.as_str())
                    && decl.sources.iter().any(|k| k.as_str() == source_kind)
                    && decl.targets.iter().any(|k| k.as_str() == target_kind)
            });
            if !declared {
                bad(
                    Some(edge.source.canonical_key()),
                    format!(
                        "edge `{source_kind}` -{}-> `{target_kind}` ({}) is not declared",
                        edge.label.as_str(),
                        edge.role.as_str()
                    ),
                );
            }
        }

        if violations.is_empty() { Ok(()) } else { Err(violations) }
    }

    /// The declaration of the children with `label` under a node of
    /// `parent_kind`, when there is one.
    fn child_decl(&self, parent_kind: &str, label: &str) -> Option<&ChildDecl> {
        self.children
            .iter()
            .find(|decl| decl.label.as_str() == label && decl.parents.iter().any(|p| p.as_str() == parent_kind))
    }

    /// The narrowest declaration the given graphs satisfy, as a starting point
    /// for writing one by hand: every observed kind, field, child group and
    /// edge, with the tightest occurrence and shape. Edge labels are exact, so
    /// families such as `jump:0`, `jump:1` need to be turned into a prefix by
    /// hand. A child group whose ordinals fit no shape is reported, not
    /// guessed.
    pub fn infer<'g>(
        recipe: &str,
        graphs: impl IntoIterator<Item = &'g Graph>,
    ) -> Result<Self, Vec<String>> {
        type Fields = BTreeMap<(String, String), (BTreeSet<Dimension>, BTreeSet<ValueType>, usize, usize, usize)>;
        let mut kinds: BTreeMap<String, usize> = BTreeMap::new();
        let mut fields: Fields = BTreeMap::new();
        let mut child_groups: BTreeMap<(String, String), (BTreeSet<String>, BTreeSet<Shape>)> = BTreeMap::new();
        // How many parent nodes carry each child group, against `kinds` totals.
        let mut group_parents: BTreeMap<(String, String), usize> = BTreeMap::new();
        let mut edges: BTreeMap<(String, String, String, EdgeRole), ()> = BTreeMap::new();
        let mut roots = BTreeSet::new();
        let mut single_root = true;
        let mut problems = Vec::new();

        for graph in graphs {
            let mut is_child = BTreeSet::new();
            for node in graph.nodes.values() {
                *kinds.entry(node.kind.as_str().to_string()).or_default() += 1;
                let mut counts: BTreeMap<&str, usize> = BTreeMap::new();
                for field in &node.fields {
                    *counts.entry(field.name.as_str()).or_default() += 1;
                    let entry = fields
                        .entry((node.kind.as_str().to_string(), field.name.as_str().to_string()))
                        .or_insert_with(|| (BTreeSet::new(), BTreeSet::new(), 0, usize::MAX, 0));
                    entry.0.insert(field.dimension);
                    entry.1.insert(ValueType::of(&field.value));
                }
                // Per-node counts, min and max, are folded in below.
                for (name, n) in counts {
                    let entry = fields.get_mut(&(node.kind.as_str().to_string(), name.to_string())).unwrap();
                    entry.2 += 1; // nodes of this kind that carry it
                    entry.3 = entry.3.min(n);
                    entry.4 = entry.4.max(n);
                }
            }
            let mut groups: BTreeMap<(&crate::key::NodeKey, &str), Vec<(u32, String)>> = BTreeMap::new();
            for child in &graph.children {
                is_child.insert(&child.child);
                let child_kind = graph.nodes[&child.child].kind.as_str().to_string();
                groups.entry((&child.parent, child.label.as_str())).or_default().push((child.ordinal, child_kind));
            }
            for ((parent, label), mut list) in groups {
                list.sort();
                let ordinals: Vec<u32> = list.iter().map(|(o, _)| *o).collect();
                let mut fits = BTreeSet::new();
                if ordinals == [0] {
                    fits.extend([Shape::One, Shape::Seq, Shape::Set]);
                } else {
                    if ordinals.iter().enumerate().all(|(i, &o)| o as usize == i) {
                        fits.insert(Shape::Seq);
                    }
                    if ordinals.iter().all(|&o| o == 0) {
                        fits.insert(Shape::Set);
                    }
                }
                let parent_kind = graph.nodes[parent].kind.as_str().to_string();
                *group_parents.entry((parent_kind.clone(), label.to_string())).or_default() += 1;
                if fits.is_empty() {
                    problems.push(format!("`{parent_kind}` -{label}-> has ordinals {ordinals:?}: no shape fits"));
                }
                let entry = child_groups
                    .entry((parent_kind, label.to_string()))
                    .or_insert_with(|| (BTreeSet::new(), [Shape::One, Shape::Seq, Shape::Set].into()));
                entry.0.extend(list.into_iter().map(|(_, k)| k));
                entry.1 = entry.1.intersection(&fits).copied().collect();
            }
            let graph_roots: Vec<_> = graph.nodes.values().filter(|n| !is_child.contains(&n.key)).collect();
            single_root &= graph_roots.len() == 1;
            roots.extend(graph_roots.iter().map(|n| n.kind.as_str().to_string()));
            for edge in &graph.edges {
                if edge.role == EdgeRole::Origin {
                    continue;
                }
                edges.insert(
                    (
                        graph.nodes[&edge.source].kind.as_str().to_string(),
                        edge.label.as_str().to_string(),
                        graph.nodes[&edge.target].kind.as_str().to_string(),
                        edge.role,
                    ),
                    (),
                );
            }
        }

        let name = |s: &str| Name::new(s, "recipe declaration").expect("non-empty name");
        let mut kind_decls = Vec::new();
        for (kind, total) in &kinds {
            let mut field_decls = Vec::new();
            for ((k, field), (dims, types, carriers, min, max)) in &fields {
                if k != kind {
                    continue;
                }
                if types.len() != 1 {
                    problems.push(format!("`{kind}.{field}` holds values of types {types:?}"));
                }
                let occurs = if *max > 1 {
                    Occurs::Many
                } else if carriers == total && *min == 1 {
                    Occurs::One
                } else {
                    Occurs::Optional
                };
                field_decls.push(FieldDecl {
                    name: name(field),
                    dimensions: dims.clone(),
                    value: *types.iter().next().expect("one type"),
                    occurs,
                });
            }
            kind_decls.push(KindDecl { kind: name(kind), fields: field_decls });
        }
        let mut children: Vec<ChildDecl> = Vec::new();
        for ((parent, label), (child_kinds, shapes)) in child_groups {
            let shape = if shapes.contains(&Shape::One) {
                Shape::One
            } else if shapes.contains(&Shape::Seq) {
                Shape::Seq
            } else if shapes.contains(&Shape::Set) {
                Shape::Set
            } else {
                problems.push(format!("`{parent}` -{label}-> fits no single shape across graphs"));
                Shape::Seq
            };
            let optional = shape == Shape::One
                && group_parents.get(&(parent.clone(), label.clone())).copied().unwrap_or(0)
                    < kinds.get(&parent).copied().unwrap_or(0);
            children.push(ChildDecl {
                parents: [name(&parent)].into(),
                label: name(&label),
                children: child_kinds.iter().map(|k| name(k)).collect(),
                shape,
                optional,
            });
        }
        let edges = edges
            .into_keys()
            .map(|(s, l, t, role)| EdgeDecl {
                sources: [name(&s)].into(),
                label: LabelPattern::Exact(name(&l)),
                targets: [name(&t)].into(),
                role,
            })
            .collect();
        if !problems.is_empty() {
            return Err(problems);
        }
        Ok(Self {
            recipe: name(recipe),
            roots: roots.iter().map(|k| name(k)).collect(),
            single_root,
            kinds: kind_decls,
            children,
            edges,
            acyclic: Acyclic::No,
            keys: String::new(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::key::{EntityKey, GraphKey, NodeKey};

    fn key(local: &str) -> NodeKey {
        NodeKey::entity(EntityKey::new("t.n", "o", local).unwrap())
    }

    fn call(args: &[(u32, u64)]) -> Graph {
        let mut g = Graph::new(GraphKey::new(EntityKey::new("t.g", "o", "g").unwrap(), "g").unwrap());
        g.add_node(key("call"), "call").unwrap();
        g.add_field(&key("call"), Dimension::Names, "callee", "f").unwrap();
        for (i, (ordinal, value)) in args.iter().enumerate() {
            let k = key(&format!("arg{i}"));
            g.add_node(k.clone(), "lit").unwrap();
            g.add_field(&k, Dimension::Constants, "value", *value).unwrap();
            g.add_child(&key("call"), "arg", *ordinal, &k).unwrap();
        }
        g
    }

    #[test]
    fn inferred_declaration_accepts_its_examples_and_rejects_others() {
        let decl = RecipeDecl::infer("t/1", [&call(&[(0, 1), (1, 2)]), &call(&[(0, 3)])]).unwrap();
        assert_eq!(decl.children[0].shape, Shape::Seq);
        assert_eq!(decl.fills(), [Dimension::Structure, Dimension::Names, Dimension::Constants].into());
        decl.check(&call(&[(0, 7), (1, 8), (2, 9)])).unwrap();

        // A gap in a sequence is refused.
        let errors = decl.check(&call(&[(0, 1), (2, 2)])).unwrap_err();
        assert!(errors[0].problem.contains("ordinals"), "{errors:?}");

        // A field in the wrong dimension is refused.
        let mut wrong = call(&[(0, 1)]);
        wrong.add_field(&key("arg0"), Dimension::Types, "value", 1u64).unwrap();
        assert!(decl.check(&wrong).is_err());
    }

    #[test]
    fn a_required_single_child_must_be_present_and_an_optional_one_may_be_absent() {
        let mut g = Graph::new(GraphKey::new(EntityKey::new("t.g", "o", "g").unwrap(), "g").unwrap());
        g.add_node(key("let"), "let").unwrap();
        g.add_node(key("ty"), "ty").unwrap();
        g.add_child(&key("let"), "type", 0, &key("ty")).unwrap();
        let mut decl = RecipeDecl::infer("t/1", [&g]).unwrap();
        assert!(!decl.children[0].optional);

        let mut bare = Graph::new(GraphKey::new(EntityKey::new("t.g", "o", "g").unwrap(), "g").unwrap());
        bare.add_node(key("let"), "let").unwrap();
        let errors = decl.check(&bare).unwrap_err();
        assert!(errors[0].problem.contains("declared required"), "{errors:?}");

        decl.children[0].optional = true;
        decl.check(&bare).unwrap();
    }

    #[test]
    fn a_declaration_that_says_one_thing_twice_is_refused() {
        let mut decl = RecipeDecl::infer("t/1", [&call(&[(0, 1)])]).unwrap();
        decl.validate().unwrap();
        let again = decl.kinds[0].clone();
        decl.kinds.push(again);
        assert!(decl.validate().unwrap_err()[0].contains("declared twice"));

        let text = serde_json::to_string(&RecipeDecl::infer("t/1", [&call(&[(0, 1)])]).unwrap()).unwrap();
        RecipeDecl::from_json(&text).unwrap();
        assert!(RecipeDecl::from_json(&text.replace("\"lit\"", "\"\"")).is_err(), "an empty name is refused on read");
    }

    #[test]
    fn field_dimensions_answer_name_checks() {
        let decl = RecipeDecl::infer("t/1", [&call(&[(0, 1)])]).unwrap();
        assert_eq!(decl.field_dimensions("lit", "value"), Some(&[Dimension::Constants].into()));
        assert_eq!(decl.field_dimensions("lit", "valu"), None);
        assert!(decl.declares_field(Dimension::Names, "callee"));
        assert!(!decl.declares_field(Dimension::Types, "callee"));
    }
}
