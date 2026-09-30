//! Function regions of an artifact, read from a region manifest, and the
//! lookup every per-function table uses.

use riff_catalog_bloat::RegionManifest;

/// Row label for bytes no function region holds.
pub const OUTSIDE_FUNCTIONS: &str = "(outside functions)";

/// One named function region, a half-open byte range.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FunctionRegion {
    pub name: String,
    pub start: u32,
    pub end: u32,
}

/// The `function` regions of a manifest, sorted by start.
#[derive(Clone, Debug, Default)]
pub struct FunctionRegions {
    regions: Vec<FunctionRegion>,
}

impl FunctionRegions {
    pub fn new(mut regions: Vec<FunctionRegion>) -> Self {
        regions.sort_by_key(|r| r.start);
        Self { regions }
    }

    /// The manifest's regions of kind `function`, in start order.
    pub fn from_manifest(manifest: &RegionManifest) -> Self {
        Self::new(
            manifest
                .regions
                .iter()
                .filter(|r| r.kind == "function")
                .map(|r| FunctionRegion {
                    name: r.name.clone(),
                    start: r.start as u32,
                    end: r.end as u32,
                })
                .collect(),
        )
    }

    pub fn iter(&self) -> impl Iterator<Item = &FunctionRegion> {
        self.regions.iter()
    }

    pub fn len(&self) -> usize {
        self.regions.len()
    }

    pub fn is_empty(&self) -> bool {
        self.regions.is_empty()
    }

    /// The region holding `pc`: the last region starting at or before it,
    /// when `pc` is before that region's end.
    pub fn at(&self, pc: u32) -> Option<&FunctionRegion> {
        let i = self.regions.partition_point(|r| r.start <= pc);
        i.checked_sub(1)
            .map(|i| &self.regions[i])
            .filter(|r| pc < r.end)
    }

    /// Name of the region holding `pc`.
    pub fn name_at(&self, pc: u32) -> Option<&str> {
        self.at(pc).map(|r| r.name.as_str())
    }

    /// Name of the region holding `pc`, or [`OUTSIDE_FUNCTIONS`].
    pub fn label_at(&self, pc: u32) -> String {
        self.name_at(pc).unwrap_or(OUTSIDE_FUNCTIONS).to_string()
    }
}
