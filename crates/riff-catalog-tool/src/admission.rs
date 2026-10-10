//! Admission: the one path by which a unit enters a corpus.

use std::collections::BTreeMap;
use std::path::Path;

use anyhow::{Context, Result};
use riff_catalog_core::{
    Digest, DigestRequest, Facet, Graph, GraphKey, HashPolicy, Keys, RecipeDecl, Status, Unit, digest_graph,
    read_units,
};

use crate::corpus::{Corpus, Record, file_safe};
use crate::declarations::Declarations;

/// An id for one batch of an application's records: the owner it files them
/// under, where the input came from (a full path or reference, so two inputs
/// with one base name stay apart), and a variant of the same input.
pub fn artifact_id(owner: &str, origin: &str, variant: bool) -> String {
    use sha2::{Digest as _, Sha256};
    let mut hasher = Sha256::new();
    hasher.update(owner.as_bytes());
    hasher.update([0x1f]);
    hasher.update(origin.as_bytes());
    hasher.update([u8::from(variant)]);
    hex::encode(hasher.finalize())[..16].to_string()
}

/// A short hash of an input's origin, for file names.
pub fn origin_hash(origin: &str) -> String {
    use sha2::{Digest as _, Sha256};
    hex::encode(Sha256::digest(origin.as_bytes()))[..8].to_string()
}

/// Admit one unit a Rust producer made, as its final answer. See
/// [`admit_unit`].
#[allow(clippy::too_many_arguments)]
pub fn emit_unit(
    declarations: &Declarations,
    records: &mut Vec<Record>,
    artifact: &str,
    owner: &str,
    level: &str,
    unit: &str,
    name: &str,
    graph_key: &GraphKey,
    graph: &Graph,
) -> Result<BTreeMap<String, Digest>> {
    if *graph_key != graph.graph_key {
        anyhow::bail!("{name}: the unit's key is not its graph's key");
    }
    admit_unit(declarations, records, artifact, owner, level, unit, name, Status::Final, graph)
}

/// Admit one unit: check it against its recipe's declaration (a recipe with
/// none is refused), hash it with
/// bound and with free keys in the dimensions the recipe fills, and append its
/// graph and digest records. Every producer, Rust or JSON, goes through here.
/// Returns the unit's all-dimensions address per keys mode.
#[allow(clippy::too_many_arguments)]
pub fn admit_unit(
    declarations: &Declarations,
    records: &mut Vec<Record>,
    artifact: &str,
    owner: &str,
    recipe: &str,
    population: &str,
    name: &str,
    status: Status,
    graph: &Graph,
) -> Result<BTreeMap<String, Digest>> {
    declarations.check(recipe, name, graph)?;
    records.push(Record::Graph {
        artifact_id: artifact.to_string(),
        unit: population.to_string(),
        level: recipe.to_string(),
        name: name.to_string(),
        status,
        graph_key: graph.graph_key.clone(),
        graph: graph.clone(),
    });
    let mut addresses = BTreeMap::new();
    for (mode, keys) in [("identity", Keys::Bound), ("shape", Keys::Free)] {
        let policy = HashPolicy::new(recipe, keys)?;
        let request = DigestRequest::new(graph.graph_key.clone(), policy.clone(), declarations.dimensions(recipe)?)?;
        let result = digest_graph(&request, graph)?;
        let facet = Facet::new(policy.policy_id(), result.hashes.graph.values.keys().copied())?;
        addresses.insert(
            mode.to_string(),
            riff_catalog_core::facet_address(&policy, graph, &result.hashes, &facet)?.address_digest(),
        );
        let joint = riff_catalog_core::joint_digests(&policy, graph, &result.hashes)?;
        records.push(Record::Digest {
            artifact_id: artifact.to_string(),
            owner: owner.to_string(),
            unit: population.to_string(),
            level: recipe.to_string(),
            name: name.to_string(),
            status,
            mode: mode.to_string(),
            policy_id: policy.policy_id(),
            digests: result.hashes.graph.values.clone(),
            shape: result.hashes.shape,
            mixed_pairs: result.hashes.mixed_pairs,
            joint,
            node_count: graph.nodes.len(),
        });
    }
    Ok(addresses)
}

/// Admit units written as JSON lines by any producer into `world`. Each file
/// is one replacement scope: admitting it again replaces what it admitted
/// before and nothing else.
pub fn admit_files(corpus: &Corpus, world: &str, files: &[impl AsRef<Path>]) -> Result<usize> {
    let mut admitted = 0;
    for file in files {
        let file = file.as_ref();
        let text = std::fs::read_to_string(file).with_context(|| format!("reading {}", file.display()))?;
        let units = read_units(&text).map_err(|error| anyhow::anyhow!("{}: {error}", file.display()))?;
        admitted += admit_units(corpus, world, &file.display().to_string(), &units)?;
    }
    Ok(admitted)
}

/// The id of the batch that [`admit_units`] files a world's scope under.
pub fn batch_id(world: &str, scope: &str) -> String {
    format!("{world}:{scope}")
}

/// Admit a complete batch of units as one replacement scope of `world`.
/// Nothing is written unless every unit passes; two units with one key are
/// refused.
pub fn admit_units(corpus: &Corpus, world: &str, scope: &str, units: &[Unit]) -> Result<usize> {
    let artifact = batch_id(world, scope);
    let mut records = vec![Record::Artifact {
        artifact_id: artifact.clone(),
        owner: world.to_string(),
        origin: scope.to_string(),
        pipeline: "units".to_string(),
        optimize: false,
        compiler: None,
        label: Some(world.to_string()),
    }];
    let mut keys = std::collections::BTreeSet::new();
    for unit in units {
        if !keys.insert((unit.recipe.as_str(), unit.population.as_str(), unit.key())) {
            anyhow::bail!("{scope}: two units of {} with the key of `{}`", unit.recipe.as_str(), unit.name);
        }
        admit_unit(
            corpus.declarations(),
            &mut records,
            &artifact,
            world,
            unit.recipe.as_str(),
            unit.population.as_str(),
            &unit.name,
            unit.status,
            &unit.graph,
        )?;
    }
    corpus.replace(&format!("units-{}", file_safe(&artifact)), &records)?;
    Ok(units.len())
}

/// Read a declaration file.
pub fn read_declaration(path: &Path) -> Result<RecipeDecl> {
    let text = std::fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
    RecipeDecl::from_json(&text).map_err(|error| anyhow::anyhow!("{}: {error}", path.display()))
}
