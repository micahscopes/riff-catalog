//! Units: what a producer hands to the catalog.
//!
//! A unit is one observation at a recipe: a graph, the population it belongs
//! to (which kind of thing it is: a function, an object, an answer to a
//! question), and a display name. Its identity across worlds is the graph's
//! own key, which must not name the world that produced it.
//!
//! A producer in any language writes units as JSON, one per line, and ships
//! the recipe's declaration as a JSON file. Nothing here depends on Rust.

use serde::{Deserialize, Serialize};

use crate::graph::Graph;
use crate::text::Name;

/// Whether an observation is the producer's final answer. A provisional
/// observation never counts as agreement in a comparison.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Status {
    #[default]
    Final,
    Provisional,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Unit {
    /// The recipe the graph was made with, e.g. `hull/1`.
    pub recipe: Name,
    /// Which kind of thing this is within the recipe, e.g. `hull-fn`.
    pub population: Name,
    /// A name to show people. Never used to pair units.
    pub name: String,
    #[serde(default)]
    pub status: Status,
    pub graph: Graph,
}

impl Unit {
    /// The key that pairs this unit with the same observation in another
    /// world: the graph's key.
    pub fn key(&self) -> String {
        self.graph.graph_key.canonical_key()
    }
}

/// Read units from JSON lines; blank lines are skipped.
pub fn read_units(text: &str) -> Result<Vec<Unit>, String> {
    text.lines()
        .enumerate()
        .filter(|(_, line)| !line.trim().is_empty())
        .map(|(index, line)| serde_json::from_str(line).map_err(|error| format!("line {}: {error}", index + 1)))
        .collect()
}

/// Write units as JSON lines.
pub fn write_units(units: &[Unit]) -> String {
    units.iter().map(|unit| serde_json::to_string(unit).expect("units serialize") + "\n").collect()
}
