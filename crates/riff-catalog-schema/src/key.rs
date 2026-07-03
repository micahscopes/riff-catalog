use serde::{Deserialize, Serialize};

use crate::error::CatalogError;
use crate::text::{Name, UNIT_SEP};

/// Stable identity of an entity that leaves a producer (compiler).
///
/// `kind` is owned by the producing phase ("hir.expr", "yul.function", …),
/// `owner` identifies the containing artifact, `local` the entity inside it.
/// Mirrors fe's `OriginExportKey` shape so fe can map 1:1 at its boundary,
/// with zero dependencies on fe (invariant: no salsa here).
///
/// `owner` and `local` accept fe's `owner_key` / `local_key` field names on
/// deserialize (serde aliases), so an fe-emitted origin key parses into this
/// type unchanged while fe renames its fields over one release. The canonical
/// serialized form is `owner` / `local`; drop the aliases once fe has renamed.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct EntityKey {
    kind: Name,
    #[serde(alias = "owner_key")]
    owner: Name,
    #[serde(alias = "local_key")]
    local: Name,
}

impl EntityKey {
    pub fn new(
        kind: impl Into<String>,
        owner: impl Into<String>,
        local: impl Into<String>,
    ) -> Result<Self, CatalogError> {
        Ok(Self {
            kind: Name::new(kind, "entity key kind")?,
            owner: Name::new(owner, "entity key owner")?,
            local: Name::new(local, "entity key local")?,
        })
    }

    pub fn kind(&self) -> &str {
        self.kind.as_str()
    }

    pub fn owner(&self) -> &str {
        self.owner.as_str()
    }

    pub fn local(&self) -> &str {
        self.local.as_str()
    }

    pub fn canonical_key(&self) -> String {
        format!(
            "{}{sep}{}{sep}{}",
            self.kind,
            self.owner,
            self.local,
            sep = UNIT_SEP
        )
    }
}

/// Key of a node inside a [`crate::Graph`]: either an exported entity, or a
/// node derived during lowering that has no producer-side identity of its own.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NodeKey {
    Entity(EntityKey),
    Derived { owner: EntityKey, local: Name },
}

impl NodeKey {
    pub fn entity(key: EntityKey) -> Self {
        Self::Entity(key)
    }

    pub fn derived(owner: EntityKey, local: impl Into<String>) -> Result<Self, CatalogError> {
        Ok(Self::Derived {
            owner,
            local: Name::new(local, "node key local")?,
        })
    }

    pub fn owner(&self) -> &EntityKey {
        match self {
            Self::Entity(key) => key,
            Self::Derived { owner, .. } => owner,
        }
    }

    pub fn canonical_key(&self) -> String {
        match self {
            Self::Entity(key) => format!("entity:{}", key.canonical_key()),
            Self::Derived { owner, local } => {
                format!("derived:{}{}{}", owner.canonical_key(), UNIT_SEP, local)
            }
        }
    }
}

/// Key of a whole graph (one lowered unit: a function, a contract, an object).
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct GraphKey {
    pub owner: EntityKey,
    pub local: Name,
}

impl GraphKey {
    pub fn new(owner: EntityKey, local: impl Into<String>) -> Result<Self, CatalogError> {
        Ok(Self {
            owner,
            local: Name::new(local, "graph key local")?,
        })
    }

    pub fn canonical_key(&self) -> String {
        format!("{}{}{}", self.owner.canonical_key(), UNIT_SEP, self.local)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn derived_keys_keep_owner_context() {
        let a = NodeKey::derived(
            EntityKey::new("mir.body", "pkg:a", "body:0").unwrap(),
            "tmp:0",
        )
        .unwrap();
        let b = NodeKey::derived(
            EntityKey::new("mir.body", "pkg:b", "body:0").unwrap(),
            "tmp:0",
        )
        .unwrap();
        assert_ne!(a, b);
        assert_ne!(a.canonical_key(), b.canonical_key());
    }

    #[test]
    fn canonical_keys_are_injective_across_variants() {
        let entity = NodeKey::entity(EntityKey::new("k", "o", "l").unwrap());
        let derived = NodeKey::derived(EntityKey::new("k", "o", "l").unwrap(), "l2").unwrap();
        assert_ne!(entity.canonical_key(), derived.canonical_key());
        assert!(entity.canonical_key().starts_with("entity:"));
        assert!(derived.canonical_key().starts_with("derived:"));
    }
}
