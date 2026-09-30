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
    /// The largest end among `regions[..=i]`, to stop a backward search.
    max_end: Vec<u32>,
}

impl FunctionRegions {
    pub fn new(mut regions: Vec<FunctionRegion>) -> Self {
        regions.sort_by_key(|r| r.start);
        let max_end = regions
            .iter()
            .scan(0u32, |m, r| {
                *m = (*m).max(r.end);
                Some(*m)
            })
            .collect();
        Self { regions, max_end }
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

    /// The region holding `pc`. When regions nest, the innermost: of the
    /// regions holding `pc`, the one that starts last.
    pub fn at(&self, pc: u32) -> Option<&FunctionRegion> {
        let mut i = self.regions.partition_point(|r| r.start <= pc);
        while i > 0 && self.max_end[i - 1] > pc {
            i -= 1;
            if pc < self.regions[i].end {
                return Some(&self.regions[i]);
            }
        }
        None
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

#[cfg(test)]
mod tests {
    use super::*;

    fn region(name: &str, start: u32, end: u32) -> FunctionRegion {
        FunctionRegion {
            name: name.into(),
            start,
            end,
        }
    }

    #[test]
    fn a_pc_belongs_to_the_innermost_region_holding_it() {
        let regions = FunctionRegions::new(vec![
            region("outer", 0, 10),
            region("inner", 2, 4),
            region("next", 10, 12),
        ]);
        let names: Vec<String> = (0..13).map(|pc| regions.label_at(pc)).collect();
        assert_eq!(
            names,
            [
                "outer",
                "outer",
                "inner",
                "inner",
                "outer",
                "outer",
                "outer",
                "outer",
                "outer",
                "outer",
                "next",
                "next",
                OUTSIDE_FUNCTIONS
            ]
        );
    }
}
