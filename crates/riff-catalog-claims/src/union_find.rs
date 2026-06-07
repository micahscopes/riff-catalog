//! Interned union-find over digests: path compression + union by rank.

use std::collections::BTreeMap;

use riff_catalog_core::Digest;

#[derive(Default)]
pub(crate) struct UnionFind {
    ids: BTreeMap<Digest, u32>,
    digests: Vec<Digest>,
    parent: Vec<u32>,
    rank: Vec<u8>,
}

impl UnionFind {
    pub fn intern(&mut self, digest: Digest) -> u32 {
        if let Some(&id) = self.ids.get(&digest) {
            return id;
        }
        let id = self.digests.len() as u32;
        self.ids.insert(digest, id);
        self.digests.push(digest);
        self.parent.push(id);
        self.rank.push(0);
        id
    }

    pub fn find(&mut self, id: u32) -> u32 {
        let mut root = id;
        while self.parent[root as usize] != root {
            root = self.parent[root as usize];
        }
        // path compression
        let mut current = id;
        while self.parent[current as usize] != root {
            let next = self.parent[current as usize];
            self.parent[current as usize] = root;
            current = next;
        }
        root
    }

    pub fn union(&mut self, a: u32, b: u32) {
        let (ra, rb) = (self.find(a), self.find(b));
        if ra == rb {
            return;
        }
        match self.rank[ra as usize].cmp(&self.rank[rb as usize]) {
            std::cmp::Ordering::Less => self.parent[ra as usize] = rb,
            std::cmp::Ordering::Greater => self.parent[rb as usize] = ra,
            std::cmp::Ordering::Equal => {
                self.parent[rb as usize] = ra;
                self.rank[ra as usize] += 1;
            }
        }
    }

    pub fn lookup(&self, digest: &Digest) -> Option<u32> {
        self.ids.get(digest).copied()
    }

    pub fn digest_of(&self, id: u32) -> Digest {
        self.digests[id as usize]
    }

    pub fn len(&self) -> usize {
        self.digests.len()
    }
}
