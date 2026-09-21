# Code as query

Use an example piece of Solidity as the query, rather than writing a detector.
The CLI captures resolved compiler ASTs, normalizes a selected syntax node and
returns matching source ranges. Optional overlap search explains shared fragments.

## Capture and search

Build `riffcat` from `crates/riff-catalog-cli`. Source capture needs an explicit
filesystem path to solc; searching a saved capture needs no compiler or network.

```sh
riffcat source-capture --solc /absolute/path/to/solc \
  first.sol second.sol --output capture.json

riffcat source-search capture.json \
  --query-file 0 --start 100 --end 180 --view bindings --overlap
```

Replace the example offsets with a nonempty UTF-8 byte range in the first file.
The selected node is the smallest complete AST node enclosing that range.
Capture output must not already exist. Each input file currently compiles
independently; imported compilation units are not supported by this command.

Capture bundles retain source, compiler input/output and compiler binary identity.
Reports retain source locations and the selected projection. They are not bug
verdicts or guarantees about executable behavior.

## Matching rules

| View | Retains | Erases |
|---|---|---|
| `names` | Names, literal tokens, ordered syntax and reference structure | Occurrence IDs and source locations |
| `bindings` | Literal tokens, ordered syntax and reference structure | Also resolved declaration/reference names |
| `bindings-ignore-literals` | Ordered syntax, reference structure, literal kind and denomination | Also literal values |

For example, renaming parameters can preserve a match under `bindings`, but
changing a repeated reference from `x` to `y` can break it. Ignoring a literal
can intentionally make `x + 7` match `x + 8`; this is not arithmetic equivalence.

Outside declaration bodies and inferred types are excluded from selected
function content. Equal functions can therefore occur in different surrounding
struct layouts. Unknown syntax is reported as unsupported, not silently erased.

## Whole and partial matches

Whole matches compare complete normalized records. With `--overlap`, an index
finds shared subtrees and returns paired source ranges, coverage in both
directions and unmatched spans. Nested or repeated fragments cannot be counted
twice by the alignment. Hash hits are checked against full records.

Partial fragments can use independent boundary substitutions. Even extensive
coverage does not prove one consistent whole-function substitution. Current
self-exclusion can also omit distinct artifacts with identical source text;
general clone/deployment retrieval needs that occurrence-identity limitation fixed.

The in-memory index is bounded to 100 files and explicit record/work budgets.
This is not a corpus-scale search service. Check unsupported and truncation
fields before interpreting a report.

## Cyclic Yul regions

`riffcat yul-capture` records experimental solc SSA CFG output.
`riffcat yul-compare` compares selected block sets with ordered operands,
result slots, phi predecessor/value pairing and boundary ports retained.
This requires the experimental compiler interface, not stock solc.

See the [exact comparison contract](../crates/riff-catalog-region/EXACT-CONTRACT.md)
and the individual command's `--help`. Exhausting the exact-search budget means
unknown, not different. Structural correspondence is not behavioral equivalence.
