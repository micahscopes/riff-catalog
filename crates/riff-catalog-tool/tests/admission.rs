//! Admission: units enter a corpus one way, whoever produced them.

use std::collections::BTreeSet;
use std::path::PathBuf;

use riff_catalog_core::{
    Dimension, EntityKey, Graph, GraphKey, Name, NodeKey, RecipeDecl, Status, Unit, write_units,
};
use riff_catalog_tool::corpus::{Corpus, Record};
use riff_catalog_tool::{admit_files, admit_units, emit_unit};

const RECIPE: &str = "toy/1";

struct Scratch(PathBuf);

impl Scratch {
    fn new(tag: &str) -> Self {
        let dir = std::env::temp_dir().join(format!("riffcat-admission-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        Self(dir)
    }

    fn corpus(&self, name: &str) -> Corpus {
        let mut corpus = Corpus::open(&self.0.join(name)).unwrap();
        corpus.declare(declaration()).unwrap();
        corpus
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// A function named `name` whose body is the operations `ops`, in order.
fn toy(name: &str, ops: &[&str]) -> Graph {
    let key = |local: String| NodeKey::entity(EntityKey::new("toy.node", name, local).unwrap());
    let mut graph = Graph::new(GraphKey::new(EntityKey::new("toy.fn", "program", name).unwrap(), "fn").unwrap());
    let root = key("fn".into());
    graph.add_node(root.clone(), "toy.fn").unwrap();
    graph.add_field(&root, Dimension::Names, "name", name).unwrap();
    for (index, op) in ops.iter().enumerate() {
        let node = key(format!("op{index}"));
        graph.add_node(node.clone(), "toy.op").unwrap();
        graph.add_field(&node, Dimension::Structure, "op", *op).unwrap();
        graph.add_child(&root, "body", index as u32, &node).unwrap();
    }
    graph
}

fn unit(name: &str, ops: &[&str]) -> Unit {
    Unit {
        recipe: Name::new(RECIPE, "recipe").unwrap(),
        population: Name::new("toy-fn", "population").unwrap(),
        name: name.to_string(),
        status: Status::Final,
        graph: toy(name, ops),
    }
}

fn declaration() -> RecipeDecl {
    let samples = [toy("a", &["add", "mul"]), toy("b", &[])];
    RecipeDecl::infer(RECIPE, samples.iter()).unwrap()
}

/// What a corpus says about its units, without the batch they came in.
fn digests(corpus: &Corpus) -> BTreeSet<String> {
    corpus
        .digest_rows_filtered(None, None)
        .unwrap()
        .into_iter()
        .map(|row| {
            format!(
                "{} {} {} {:?} {:?} {} {:?}",
                row.level, row.unit, row.name, row.status, row.digests, row.mixed_pairs, row.joint
            ) + &format!(" {}", row.mode)
        })
        .collect()
}

#[test]
fn a_rust_producer_and_a_json_producer_admit_the_same_thing() {
    let scratch = Scratch::new("same");
    let units = vec![unit("f", &["add", "mul"]), unit("g", &["mul"])];

    let from_rust = scratch.corpus("rust");
    admit_units(&from_rust, "w", "batch", &units).unwrap();

    let file = scratch.0.join("units.jsonl");
    std::fs::write(&file, write_units(&units)).unwrap();
    let from_json = scratch.corpus("json");
    assert_eq!(admit_files(&from_json, "w", &[&file]).unwrap(), 2);

    // The application path: a Rust lowering handing graphs to `emit_unit`.
    let from_emit = scratch.corpus("emit");
    let mut records = Vec::new();
    for unit in &units {
        emit_unit(
            from_emit.declarations(),
            &mut records,
            "artifact",
            "w",
            RECIPE,
            "toy-fn",
            &unit.name,
            &unit.graph.graph_key,
            &unit.graph,
        )
        .unwrap();
    }
    from_emit.replace("artifact", &records).unwrap();

    assert_eq!(digests(&from_rust).len(), 4, "two units, bound and free keys");
    assert_eq!(digests(&from_rust), digests(&from_json));
    assert_eq!(digests(&from_rust), digests(&from_emit));
}

#[test]
fn a_failed_batch_leaves_the_corpus_as_it_was() {
    let scratch = Scratch::new("failed");
    let corpus = scratch.corpus("corpus");
    admit_units(&corpus, "w", "batch", &[unit("f", &["add"])]).unwrap();
    let before = digests(&corpus);

    let mut broken = unit("h", &["add"]);
    let root = broken.graph.nodes.keys().next().unwrap().clone();
    broken.graph.add_field(&root, Dimension::Types, "undeclared", "x").unwrap();
    let error = admit_units(&corpus, "w", "batch", &[unit("g", &["mul"]), broken]).unwrap_err();
    assert!(error.to_string().contains("breaks the toy/1 declaration"), "{error}");
    assert_eq!(digests(&corpus), before);

    // Two units with one key are refused as well.
    let error = admit_units(&corpus, "w", "batch", &[unit("g", &["mul"]), unit("g", &["add"])]).unwrap_err();
    assert!(error.to_string().contains("two units"), "{error}");
    assert_eq!(digests(&corpus), before);
}

#[test]
fn admitting_a_scope_again_replaces_it_and_nothing_else() {
    let scratch = Scratch::new("replace");
    let corpus = scratch.corpus("corpus");
    let first = [unit("f", &["add"]), unit("g", &["mul"])];
    admit_units(&corpus, "w", "one", &first).unwrap();
    admit_units(&corpus, "w", "two", &[unit("h", &["sub"])]).unwrap();
    let both = digests(&corpus);
    assert_eq!(both.len(), 6);

    // Again: no change.
    admit_units(&corpus, "w", "one", &first).unwrap();
    assert_eq!(digests(&corpus), both);
    assert_eq!(corpus.digest_rows_filtered(None, None).unwrap().len(), 6);

    // A smaller batch at the same scope replaces the larger one; the other
    // scope, and the same scope name in another world, are untouched.
    admit_units(&corpus, "v", "one", &first).unwrap();
    admit_units(&corpus, "w", "one", &first[..1]).unwrap();
    let rows = corpus.digest_rows_filtered(None, None).unwrap();
    let names = |owner: &str| -> Vec<String> {
        let mut names: Vec<String> =
            rows.iter().filter(|row| row.owner == owner && row.mode == "shape").map(|row| row.name.clone()).collect();
        names.sort();
        names
    };
    assert_eq!(names("w"), ["f", "h"]);
    assert_eq!(names("v"), ["f", "g"]);
}

#[test]
fn scope_names_that_differ_only_in_punctuation_do_not_share_a_file() {
    let scratch = Scratch::new("scopes");
    let corpus = scratch.corpus("corpus");
    admit_units(&corpus, "w", "a/b", &[unit("f", &["add"])]).unwrap();
    admit_units(&corpus, "w", "a_b", &[unit("g", &["mul"])]).unwrap();
    assert_eq!(digests(&corpus).len(), 4);
}

#[test]
fn a_corpus_keeps_the_declarations_its_units_were_admitted_under() {
    let scratch = Scratch::new("stored");
    {
        let corpus = scratch.corpus("corpus");
        admit_units(&corpus, "w", "batch", &[unit("f", &["add"])]).unwrap();
    }
    // A later run that declares nothing still checks against the stored one.
    let mut reopened = Corpus::open(&scratch.0.join("corpus")).unwrap();
    assert_eq!(reopened.declarations().get(RECIPE), Some(&declaration()));
    let mut broken = unit("h", &["add"]);
    let root = broken.graph.nodes.keys().next().unwrap().clone();
    broken.graph.add_field(&root, Dimension::Types, "undeclared", "x").unwrap();
    admit_units(&reopened, "w", "more", &[broken.clone()]).unwrap_err();

    // A different declaration of the same recipe is refused, so the unit
    // above cannot be let in by widening the declaration in place.
    let wider = RecipeDecl::infer(RECIPE, [toy("a", &["add"]), broken.graph.clone()].iter()).unwrap();
    assert_ne!(wider, declaration());
    let error = reopened.declare(wider).unwrap_err();
    assert!(format!("{error:#}").contains("two different declarations"), "{error:#}");
    reopened.declare(declaration()).unwrap();
}

#[test]
fn units_of_a_recipe_with_no_declaration_are_refused() {
    let scratch = Scratch::new("undeclared");
    let corpus = Corpus::open(&scratch.0.join("corpus")).unwrap();
    let error = admit_units(&corpus, "w", "batch", &[unit("f", &["add"])]).unwrap_err();
    assert!(error.to_string().contains("no declaration for recipe toy/1"), "{error}");
    assert!(corpus.load_non_graph_records().unwrap().is_empty());
}

#[test]
fn a_provisional_unit_keeps_its_status() {
    let scratch = Scratch::new("status");
    let corpus = scratch.corpus("corpus");
    let mut provisional = unit("f", &["add"]);
    provisional.status = Status::Provisional;
    admit_units(&corpus, "w", "batch", &[provisional]).unwrap();
    let rows = corpus.digest_rows_filtered(None, None).unwrap();
    assert!(rows.iter().all(|row| row.status == Status::Provisional));
    let stored = corpus.load_non_graph_records().unwrap();
    assert!(stored.iter().any(|record| matches!(record, Record::Declaration { .. })));
}
