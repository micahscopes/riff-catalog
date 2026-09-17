# Exact incidence contract, experimental

`exact::Graph` is a finite directed labelled multigraph. Vertex labels and edge
roles are exact strings; edge order and vertex indices are transport details.
Duplicate edges count. Ports, result ordinals, phi pairings and boundaries must
be faithfully encoded by the producer. This module cannot recover missing facts.

Canonicalization sorts labels into initial cells, refines using both incoming
and outgoing labelled incidence with multiplicities, then individualizes every
possibility in a deterministically chosen tied cell. Discrete leaves yield full
relabelled graphs; the smallest completed leaf is retained. A completed result
includes a checkable occurrence-to-canonical bijection. It is not necessarily
the smallest encoding among every permutation, only among this invariant search.

## Guarantee argument

This is a proof sketch, not a machine-checked proof:

1. An isomorphism preserves initial labels and each incoming/outgoing signature,
   so refinement partitions correspond under every isomorphism.
2. Choosing the first tied color cell is invariant. Branching over every member
   yields corresponding search subtrees under every isomorphism.
3. Thus isomorphic inputs have the same set of completed labelled leaf graphs,
   and the same minimum. Occurrence mappings may differ under automorphisms.
4. Conversely, equal canonical graphs plus their verified bijections supply an
   isomorphism of the inputs. Labels, directions, roles and multiplicities are
   all checked, not merely a digest or color histogram.
5. Each individualization strictly increases the number of cells. Recursion
   depth is bounded by vertex count; exhaustive completion is finite. Operational
   limits may stop it earlier without issuing a canonical result.

Limits default to 64 vertices, 256 edges, 100,000 search states, two seconds and
1 MiB combined labels. Graph-size or search exhaustion returns BudgetExceeded,
never an exact partial answer. Invalid endpoints return Unsupported. A timeout
changes availability, not a completed canonical graph's meaning.

The scalar `Graph` order is Rust's explicitly derived sequence/lexical order.
There is no persistent address format yet for this experimental core. Any future
content address needs a versioned byte encoding and domain. This does not change
old Riff-cat SCC hashes.

## Tests and remaining producer work

- All 64 loop-free directed graphs on three vertices, all 4096 pair comparisons,
  checked against independent permutation enumeration.
- Transport renaming, directions, labels, edge roles, multiplicity and bad mappings.
- Triangular prism versus K3,3: tied WL colors, distinct exact canonical graphs.
- Budget exhaustion is explicit.

These tests exercise a general reference core. Wiring real Yul result slots,
phi-predessor associations and selected boundaries is still required before the
cyclic-Yul application slice can be claimed complete.
