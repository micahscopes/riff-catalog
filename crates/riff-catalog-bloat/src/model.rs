use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

pub const SCHEMA_VERSION: &str = "riff-catalog-bloat/2";
pub const LEGACY_SCHEMA_VERSION: &str = "riff-catalog-bloat/1";

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CaptureFile {
    pub schema: String,
    pub capture_id: String,
    pub capture: Capture,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Capture {
    pub label: String,
    pub provenance: Provenance,
    pub alignment: Alignment,
    #[serde(default, skip_serializing_if = "CaptureCompletion::is_legacy_unknown")]
    pub completion: CaptureCompletion,
    #[serde(default, skip_serializing_if = "Intervention::is_none")]
    pub intervention: Intervention,
    #[serde(default)]
    pub artifacts: Vec<Artifact>,
    pub stages: Vec<Stage>,
    #[serde(default)]
    pub inline_events: Vec<InlineEvent>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub clone_observations: Vec<CloneObservation>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub decisions: Vec<Decision>,
    #[serde(default)]
    pub compatibility_observations: Vec<CompatibilityObservation>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Provenance {
    pub producer: String,
    pub producer_revision: String,
    pub command: String,
    #[serde(default)]
    pub environment: BTreeMap<String, String>,
    #[serde(default)]
    pub notes: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Alignment {
    pub source_id: String,
    pub compiler_id: String,
    #[serde(default)]
    pub settings: BTreeMap<String, String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Artifact {
    pub id: String,
    pub role: ArtifactRole,
    pub path: String,
    pub blake3: String,
    pub bytes: u64,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub producer_digests: BTreeMap<String, String>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "snake_case", deny_unknown_fields)]
pub enum CaptureCompletion {
    Complete {
        producer_marker: String,
    },
    Failed {
        message: String,
        last_stage: Option<String>,
    },
    Incomplete {
        reason: String,
    },
    #[default]
    LegacyUnknown,
}

impl CaptureCompletion {
    pub fn is_legacy_unknown(&self) -> bool {
        matches!(self, Self::LegacyUnknown)
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Intervention {
    pub kind: String,
    #[serde(default)]
    pub requested: Vec<String>,
    #[serde(default)]
    pub resolved: Vec<FunctionRef>,
    #[serde(default)]
    pub consequential: Vec<FunctionRef>,
}

impl Intervention {
    pub fn none() -> Self {
        Self {
            kind: "none".into(),
            requested: Vec::new(),
            resolved: Vec::new(),
            consequential: Vec::new(),
        }
    }
    pub fn is_none(&self) -> bool {
        self.kind.is_empty() || self.kind == "none"
    }
}

impl Default for Intervention {
    fn default() -> Self {
        Self::none()
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ArtifactRole {
    CompilerTrace,
    Source,
    Ir,
    EmittedWgsl,
    Other,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Stage {
    pub id: String,
    pub kind: String,
    #[serde(default)]
    pub predecessors: Vec<String>,
    #[serde(default)]
    pub functions: Vec<Function>,
    #[serde(default)]
    pub selected_entries: Vec<String>,
    #[serde(default)]
    pub direct_calls: Vec<DirectCall>,
    #[serde(default)]
    pub call_graph: CallGraphCompleteness,
    #[serde(default)]
    pub unknown_indirect_calls: Vec<UnknownIndirectCall>,
    #[serde(default)]
    pub measurements: Vec<Measurement>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "completeness", rename_all = "snake_case", deny_unknown_fields)]
pub enum CallGraphCompleteness {
    Complete,
    Incomplete { reason: String },
}

impl Default for CallGraphCompleteness {
    fn default() -> Self {
        Self::Incomplete {
            reason: "producer did not declare call-graph completeness".into(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Function {
    /// Identity is local to the containing stage.
    pub id: String,
    pub display_name: String,
    pub instructions: u64,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DirectCall {
    pub caller: String,
    pub callee: String,
    pub callsites: u64,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UnknownIndirectCall {
    pub caller: String,
    pub callsites: u64,
    pub reason: String,
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(tag = "scope", rename_all = "snake_case", deny_unknown_fields)]
pub enum MeasurementScope {
    RootBody { entry: String },
    SelectedEntryBodies { entries_known: bool },
    ReachableUnion { entries: Vec<String> },
    AllModule,
    FunctionBody { function: String },
    Artifact { artifact: String },
    TraceSegment,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "unit", content = "value", rename_all = "snake_case")]
pub enum Quantity {
    Instructions(u64),
    Functions(u64),
    Callsites(u64),
    Bytes(u64),
    Nanoseconds(u64),
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Evidence {
    CompilerEvent { producer: String },
    ArtifactMeasurement { artifact: String },
    DerivedCallGraph { stage: String },
    CompatibilityTrace { artifact: String, grammar: String },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Measurement {
    pub name: String,
    pub scope: MeasurementScope,
    pub quantity: Quantity,
    pub evidence: Evidence,
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FunctionRef {
    pub stage: String,
    pub function: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct InlineEvent {
    pub id: String,
    pub caller: FunctionRef,
    pub callee: FunctionRef,
    pub output_stage: String,
    pub callsites: u64,
    /// Total cloned instructions across all represented callsites.
    pub cloned_instructions: u64,
    /// Literal original instruction IDs still inserted at the observation
    /// stage. This is not a count of rewritten descendants.
    pub surviving_original_ids: Option<u64>,
    pub evidence: Evidence,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CloneObservation {
    pub observation_stage: String,
    pub compiler_frontier: u64,
    pub compiler_stage: String,
    pub caller: FunctionRef,
    pub callee: FunctionRef,
    pub callsites: u64,
    pub cloned_instructions: u64,
    pub surviving_original_ids: u64,
    pub evidence: Evidence,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Decision {
    pub stage: String,
    pub subject: Option<FunctionRef>,
    pub decision: DecisionKind,
    pub evidence: Evidence,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum DecisionKind {
    ExactFunctionMerge {
        candidate_functions: u64,
        merged_functions: u64,
        rewritten_references: u64,
        refinement_rounds: u64,
    },
    BackendCallable {
        variants: u64,
        instructions: u64,
        accesses_resource: bool,
        maximum_physical_parameters: u64,
    },
    BackendRejected {
        reason: String,
    },
    BaselineRetained,
    FrontendRetained,
    ForcedInline,
    ConsequentialInline,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum CompatibilityObservation {
    HelperClones {
        segment: u64,
        observation_stage: String,
        compiler_frontier: u64,
        compiler_stage: String,
        callee_name: String,
        callsites: u64,
        cloned_instructions: u64,
        surviving_original_ids: u64,
        artifact: String,
        line: u64,
    },
    UnknownLine {
        segment: u64,
        line: u64,
        text: String,
        artifact: String,
    },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Reachability {
    pub entries: Vec<String>,
    pub functions: Vec<String>,
    pub instructions: u64,
    pub complete: bool,
    pub unknown_indirect_callsites: u64,
}
