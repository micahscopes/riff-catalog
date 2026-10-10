//! Serialize `BTreeMap<K, V>` with non-string keys as a JSON array of pairs.
//! (serde_json rejects struct-keyed maps; JSONL corpora need plain JSON.)

use std::collections::BTreeMap;

use serde::{Deserialize, Deserializer, Serialize, Serializer};

pub fn serialize<K, V, S>(map: &BTreeMap<K, V>, serializer: S) -> Result<S::Ok, S::Error>
where
    K: Serialize + Ord,
    V: Serialize,
    S: Serializer,
{
    serializer.collect_seq(map.iter())
}

pub fn deserialize<'de, K, V, D>(deserializer: D) -> Result<BTreeMap<K, V>, D::Error>
where
    K: Deserialize<'de> + Ord,
    V: Deserialize<'de>,
    D: Deserializer<'de>,
{
    // A repeated key is an error, as it is when a map is built in Rust: taking
    // the last pair silently would let two different inputs mean one map.
    let pairs = Vec::<(K, V)>::deserialize(deserializer)?;
    let mut map = BTreeMap::new();
    for (key, value) in pairs {
        if map.insert(key, value).is_some() {
            return Err(serde::de::Error::custom("duplicate key in a pair map"));
        }
    }
    Ok(map)
}
