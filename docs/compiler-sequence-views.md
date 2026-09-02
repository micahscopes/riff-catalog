# Compiler sequence views

Compiler investigations need to compare two related structures:

1. The lowering DAG inside one build.
2. The DAG of compiler worlds in which those lowerings were produced.

Neither structure is necessarily a list. A pass can consume several prior
artifacts, emit several successors, or be omitted by one pipeline. Compiler
worlds can branch by revision, target, profile, feature set, pass policy, or
backend without forming one linear history. Git ancestry is useful provenance,
but it is not the whole comparison model.

The resulting space is a ragged product of two DAGs. Riffcat should preserve
that shape instead of flattening it into adjacent before and after rows.

## Keep five concerns distinct

### Semantic digest dimensions

The existing closed dimensions remain:

- structure
- names
- constants
- types
- trace events

They answer whether two artifacts have the same content at a selected semantic
facet. A compiler revision, build profile, or elapsed time must not enter these
digests. Otherwise identical IR produced by two compiler worlds would cease to
match.

Adding a semantic dimension remains a schema change. Most compiler questions
below need a projection, coordinate, relation, or measurement instead.

### Analysis levels and projections

An analysis level selects the portion of an artifact relevant to one question.
Useful compiler projections include:

- control topology: regions, branches, loops, joins, and structurization clones
- representation pressure: aggregate construction, projection, flattening, and
  materialization sites
- memory effects: address spaces, allocations, loads, stores, checkpoints,
  rewinds, lifetimes, and resource access
- call boundaries: direct calls, indirect calls, ABI values, result shapes, and
  call-graph closure
- helper eligibility: accepted helpers and exact rejection causes such as
  memory effects, aggregate values, result transport, recursion, or unsupported
  callees
- specialization: source definition, runtime instance, witness or assumption
  context, monomorphization origin, and exact-merge class
- placement: invocation, subgroup, workgroup, dispatch, buffer, recomputation,
  and spill decisions
- emitted footprint: statements, expressions, functions, declarations, private
  storage, and backend output regions

These should be separately versioned levels over the same closed semantic
dimensions. For example, `sonatina-helper-abi/1` can expose why a function was
inlined without changing what `structure+types+constants` means.

### Compiler-world coordinates

A compiler world describes how an observation was produced:

- source digest
- Fe revision
- Sonatina revision
- target and backend
- build profile
- enabled features
- compiler flags
- pass-pipeline identity
- relevant dependency revisions

Coordinates are provenance, not semantic content. They identify a world and
allow queries such as "same stage under these two Sonatina branches" while
leaving artifact equivalence content-addressed.

Worlds form a DAG. A relation can record that one world changed only a
Sonatina revision, forked a pass policy, or enabled one feature. Git revisions
are one useful coordinate and one possible ancestry relation, not a required
linear axis.

### Causal relations

Stage and world nodes need explicit, typed edges:

- lowered from
- optimized from
- emitted from
- aligned with
- variant derived from
- inlined because
- materialized because
- rejected because
- merged into
- retained because

Origin edges remain inert to semantic facet addresses. Causal payload can name
the introducing phase, policy rule, or eligibility failure without making two
otherwise identical artifacts unequal.

### Measurements

Measurements are observations, not identity:

- functions, blocks, instructions, calls, and graph nodes
- distinct and repeated structural classes
- emitted bytes and declarations
- private heap, workgroup storage, registers, and resource bindings
- compile time and per-phase time
- peak resident memory
- cache hit or miss state
- runtime duration on a named device and driver

They belong on a stage observation with units and measurement provenance. Two
byte-identical artifacts can have different compile times, and two differently
formatted artifacts can have identical behavior.

## Alignment across the ragged product

Phase names alone are not alignment keys. A robust comparison uses, in order:

1. Explicit derivation and alignment edges emitted by the compiler.
2. Stable source-origin keys and analysis-level identity.
3. Semantic facet addresses at the selected level.
4. A witnessed claim when equality is not established by hashing.

This supports several queries without pretending that every pipeline has the
same stages:

- Find the first aligned stage whose semantic address diverges.
- Find the first stage whose size grows even though its semantic address does
  not change.
- Attribute downstream growth to a helper rejection or materialization edge.
- Compare one stage across sibling Sonatina branches or profile variants.
- Follow one semantic subgraph through Fe RMIR, Sonatina IR, Naga, WGSL, and
  SPIR-V when origin evidence exists.
- Separate novel computation from copied structure at every stage.

## Current Mandelbrot proof example

The round-interaction kernel gives a concrete first corpus:

| stage | functions | instructions | calls | notable result |
| --- | ---: | ---: | ---: | --- |
| before exact private-function folding | 389 | 6,934 | unknown | duplicated runtime instances are present |
| after exact folding, before inlining | 136 | 3,179 | 608 | 253 functions and 3,755 instructions removed |
| after forced inlining and cleanup | 136 | 11,490 total, 8,314 in the root | 1,514 | helper ABI rejection expands the root |
| browser WGSL | n/a | n/a | n/a | 1,775,047 bytes, parses and validates, exceeds the 1 MB gate |

The helper classifier currently accepts 75 reachable helpers containing 673
instructions and rejects 57 containing 2,449 instructions. Rejection incidence
is dominated by memory effects and non-scalar values. This is a model case for
a `sonatina-helper-abi/1` projection connected to the pre-inline and post-inline
stages by `inlined because` edges.

The table does not claim semantic correctness. Browser WGSL parsing and Naga
validation establish representation validity. Independent behavior and
mutation gates establish the relevant correctness claims.

## Minimal implementation slice

1. Introduce first-class compiler-world coordinates and typed world-derivation
   edges in corpus records.
2. Generalize the current linear `ModuleTimeline` into a stage DAG while
   retaining the compact adjacent-delta convenience API.
3. Add `sonatina-helper-abi/1` and emitted-footprint projections.
4. Ingest Fe and Sonatina observations with explicit stage derivation edges.
5. Add queries for aligned first divergence, first growth, and causal growth
   propagation across selected compiler worlds.
6. Gate every optimization conclusion with independent behavior or mutation
   attestations. Digest or byte equality alone is never correctness evidence.

The compiler should emit stable observations at its boundary. It should not
depend on Riffcat for optimization decisions or cache keys. Riffcat remains a
read-only analysis and comparison tool.
