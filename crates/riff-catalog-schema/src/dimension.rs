use serde::{Deserialize, Serialize};

/// The closed set of digest dimensions.
///
/// Closed on purpose: open dimension strings would be encoding-drift bait
/// ("literal" vs "constants" between two producers). Adding a dimension is
/// deliberately a the schema version bump.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Dimension {
    Structure,
    Names,
    Constants,
    Types,
    TraceEvents,
}

impl Dimension {
    pub const ALL: [Self; 5] = [
        Self::Structure,
        Self::Names,
        Self::Constants,
        Self::Types,
        Self::TraceEvents,
    ];

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Structure => "structure",
            Self::Names => "names",
            Self::Constants => "constants",
            Self::Types => "types",
            Self::TraceEvents => "trace_events",
        }
    }

    pub fn parse(text: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|d| d.as_str() == text)
    }
}
