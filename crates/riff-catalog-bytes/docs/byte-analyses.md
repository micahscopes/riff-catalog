# Where an EVM runtime's bytes come from

`riffcat-bytes` joins an EVM runtime to its compiler's own records (a Fe
trace bundle and attribution details, a solc source map, Sonatina IR) and to
its lifted dataflow, and puts every byte in a table that adds up to the
artifact. The census of repeated EVM runs it reuses is the one in
`riff-catalog-bloat` (see [the artifact census](../../riff-catalog-bloat/docs/artifact-census.md),
section "EVM bytecode runs").

```sh
cargo build -p riff-catalog-bytes -p riff-catalog-bloat
BYTES=target/debug/riffcat-bytes
RIFFCAT=target/debug/riffcat-bloat
```

A shared address is structural correspondence at a facet, never a proof that
two pieces of code behave alike. Bytes beyond a first copy are not savings:
sharing has its own call, return and parameter costs.

## Inputs belong together or the command refuses

Every command that takes an artifact with another input checks that they
describe the same code, and refuses otherwise:

| Input | Check |
| --- | --- |
| Fe trace and attribution details | every details row names an instruction extent of the contract's runtime code object and every extent has a row; each instruction's opcode, length and PUSH immediate match the artifact byte at its pc; when the trace has a code hash, the blake3 of the whole artifact must equal it. Without a code hash, bytes after the last instruction are refused; with one they are reported as data after the code. |
| attribution details without a trace | the contract's rows are exactly the instructions the code decodes to, from pc 0 to the code end (there are no opcodes to compare) |
| region manifest | schema `riffcat-regions/1` or `/2`, and its `artifact_blake3` is the artifact's blake3 |
| census (`census --json` or `census --output`) | a census of this artifact made from a manifest with `evm_runs` (`riffcat-artifact-census/2`) |
| pc sets and named causes | every pc starts an instruction before the code end |
| saved reports read back | the expected `schema`; a byte-cause ledger must also cover its own artifact |

Artifact files are read with `--artifact-format auto` by default: hex text
(an optional `0x`, whitespace and line breaks ignored) when the file is only
hex digits, else raw bytes. Pass `hex` or `raw` to say which. The code end
(`--code-end`) defaults to the start of the manifest's `data` region, else to
the start of a solc CBOR metadata trailer, else to the end of the artifact,
and may not pass the artifact.

## Fe contracts: `fe-trace-bytes`

```sh
fe dev trace emit INGOT -O 1 --out trace.jsonl
fe dev debug emit --format ethdebug --from trace.jsonl --out ethdebug.json \
  --attribution-details attribution.json
"$BYTES" fe-trace-bytes --trace trace.jsonl --attribution attribution.json \
  --contract NAME --artifact NAME.runtime.bin --run-key exact \
  --artifact-out runtime.bin --manifest-out regions.json --json-out report.json
"$RIFFCAT" census runtime.bin --regions regions.json   # replays the run census
```

`NAME.runtime.bin` is the contract's runtime bytecode (hex text or raw
bytes); it must be the runtime the trace was emitted for, which the command
checks. The Fe commands and flags above were checked against Fe at commit
`1b64245`. The formats read here are Fe's trace bundle schema versions 1 and 2
and the attribution details `fe-ethdebug-attribution-details-v1`. Fe's own
help calls the attribution details experimental, with no compatibility
promise; a new schema version is refused until it is read here. The test
fixtures are synthetic files in these formats (see `tests/fixtures/README.md`).

Attribution is Fe's `PrimarySourceV1` decision from the details file, not
re-derived here. Tables (report `riffcat-fe-trace-bytes/2`):

- by Fe classification, confidence and reason; by emitted function (final
  code layout, from `bytecode.pc -> evm.vcode.inst`); by primary source body
  and file; by recv arm, with and without the functions only one arm reaches.
  Each adds up to the artifact length; data after the code is a row of its
  own.
- by source body anywhere among a byte's origins. These overlap by design.

Emitted functions by position: a function's region runs from its first to its
last linked byte. The unlinked gap directly before it joins it when the gap
starts with a JUMPDEST and its other JUMPDESTs are reached only from the gap
or the function (the entry JUMPDEST and argument set-up carry no vcode link).
Other gaps stay "between functions". This is a positional heuristic; no test
here compares it with the compiler's pc map. When the trace interleaves two
functions, each maximal same-function range is its own region.

Recv arms: an arm left as its own function (`__<Contract>_recv_<r>_<a>`)
owns that function. An arm inlined into the dispatcher owns the
post-optimization blocks dominated by its region entry, the nearest common
dominator of the blocks whose instructions come only from that arm. The
assignment is dropped, with a note, if the region would contain a block that
comes only from another arm. Functions each arm reaches come from calls
inferred from label pushes whose value is a function's first byte; computed
jumps are not seen, so the graph is declared incomplete.

Runs (`--run-key exact | memory-offsets-blind | constants-blind`) are joined
back per occurrence: emitted function, recv arm, the source body with the most
primary-attributed bytes, and how many instructions have the same primary
source in every copy.

## Fe compiler stages: `fe-trace-stages`

```sh
"$BYTES" fe-trace-stages --trace trace.jsonl --attribution attribution.json \
  --contract NAME --artifact NAME.runtime.bin --regions regions.json \
  --census census.json --pattern load=6040 --pc-set tags=tags.json \
  --clamp-pattern 6040516080 --spill-pcs spills.json \
  --library 'ABI=$abi$|calldata' --json-out stages.json
```

Reads the trace's origin graph by stage (HIR, MIR, Sonatina pre-opt,
post-opt, prepared, vcode, bytecode) and follows each selection of emitted
bytes back to HIR: nodes per stage, edge phases, where provenance ends,
post-opt operations, MIR forms and instances, and memory operations by
mechanism. Selections are all bytes, memory operations, bytes with no source,
each `--pattern`, each `--pc-set`, the `--runs` largest run classes of a
`--census`, and every function starting with a `--function-prefix` with its
call sites.

The mechanism of a memory operation is the first rule that holds: inside a
`--clamp-pattern` match (free-pointer clamp), in `--spill-pcs` (spill stores
and reloads), provenance ending after post-opt, a post-opt `evm_malloc`, a
copy (`copy_into`, `evm_mcopy`, `memzero`), a stack object (`obj.*`), an
explicit `mload`/`mstore`, or call and return transport. With `--library
LABEL=name|name` a bucket also names the first library whose names occur in
the operation's primary body or MIR instance; the command knows no library
names of its own. `--mechanisms-out DIR` writes one pc set per mechanism for
`evm-byte-causes`, and an `index.json` (schema `riffcat-fe-mechanisms/1`)
naming each file and the code it describes.

Chains from each HIR construct to its bytes are content addressed at level
`fe-stage-chain/1`; constructs with one address expanded the same way through
every stage. Report schema `riffcat-fe-stages/2`.

## solc contracts: `solc-functions`

```sh
solc --standard-json < input.json > output.json   # with ast and deployedBytecode.sourceMap
"$BYTES" solc-functions --standard-output output.json --source a.sol --contract A \
  --artifact-out a.bin --manifest-out a-regions.json --json-out a.json --min-run-bytes 32
```

Attributes every runtime instruction to the innermost Solidity function or
modifier, generated Yul function, contract, file or no source, from the
source map and ASTs. The source map must have one entry per instruction (solc
leaves a final INVALID without one); a malformed field is an error. The
manifest's `function` regions are the maximal runs with one owner, and the
CBOR metadata is a `data` region. Overloaded functions share one name.
Report schema `riffcat-solc-functions/1`.

## Sonatina IR: `sonatina-functions`

```sh
"$BYTES" sonatina-functions --ir module.sntn --regions regions.json --json-out son.json
```

Groups a module's functions at three facets: exact (structure, types and
constants), types blind, and types and constants blind. A global a function
reads counts as one of its constants. Emitted bytes come from `function`
regions with the same names. `upper_bound_saving` is the bytes beyond the
largest copy of a class, before any cost of making code generic. Report
schema `riffcat-sonatina-functions/1`.

## Dataflow facets: `evm-dataflow`, `evm-dataflow-compare`

```sh
"$BYTES" evm-dataflow --artifact a.bin --regions a-regions.json \
  --blocks-out a-blocks.json --json-out a-dataflow.json
"$BYTES" evm-dataflow-compare --left fe-blocks.json --right a-blocks.json --json-out cmp.json
```

Lifts every basic block into an `evm-dataflow/2` graph
(`riff_catalog_evm::dataflow`) and addresses it at eight facets: flat exact,
flat constants blind and flat memory-offsets blind (`evm-run/1` graphs of the
block's instructions), dataflow exact and dataflow constants blind, and three
`riffcat-view/1` plans over the dataflow graph that erase
`constants.memory_offset`, `structure.slot` (which entry stack slot each input
came from; outputs keep their order), or both. A constant is a memory offset
when every use of it is a memory or calldata address, directly or through
ADDs that are themselves only addresses. A PUSH1..PUSH4 whose value is a
JUMPDEST pc is read as a label; its value stays a constant at the facets that
keep constants. The report counts DUP, SWAP and POP bytes (with
`--attribution`, also those in bytes with no source) and whole-block classes
per facet. The compare lists blocks two artifacts share at each facet both
define the same way. Schemas `riffcat-evm-dataflow-blocks/2`,
`riffcat-evm-dataflow-report/2`, `riffcat-evm-dataflow-compare/1`.

## Excess ledgers: `evm-byte-causes`, `evm-byte-causes-compare`

```sh
"$BYTES" evm-byte-causes --artifact fe.bin --regions fe-regions.json \
  --cause pcs:spill=spills.json --cause 'repeats:repeats=census.json' \
  --cause 'duplicates:duplicates=son.json#0' --cause 'regions:abi=abi_' \
  --cause 'bodies:abi=$abi$' --attribution attribution.json --contract NAME \
  --detail pcs:mechanism=mech/mechanism-0.json --json-out fe-causes.json
"$BYTES" evm-byte-causes-compare --left fe-causes.json --right a-causes.json
```

Puts every byte in one bucket: the named causes in the order given (the first
that holds an instruction wins), then role buckets by opcode, then the data
after the code. Cause kinds: `pcs:` a pc set, `pattern:` a byte pattern,
`repeats:` run occurrences beyond each class's first copy (from a census),
`duplicates:` functions beyond the largest copy of each class (from a
`sonatina-functions` facet), `regions:` function regions whose name contains
one of the names, `bodies:` instructions whose Fe primary source (or
synthetic-for source) body contains one of the names. Names must be distinct
and not those of role buckets. `--detail` cross-tabulates each bucket by the
first detail set holding the instruction. Role buckets describe what leftover
bytes do; they are not causes. Ledger schema `riffcat-evm-byte-causes/2`.

The compare checks each ledger (its buckets add up to its artifact) and then
subtracts them bucket by bucket; the differences add up to the size
difference. With details on the left, it also prints an estimate that splits
each role bucket's excess over the left side's detail labels in proportion to
their bytes: an estimate, not a measurement. Schema
`riffcat-evm-byte-causes-compare/1`.

## Fe against solc per function: `compare-functions`

```sh
"$BYTES" compare-functions --fe-stages stages.json \
  --solc viair=a-viair.json --solc legacy=a-legacy.json \
  --pairs pairs.json --fe-total 24576 --json-out functions.json
```

`pairs.json` is a hand-made list of pairs:

```json
[
  {"label": "decode order", "fe": ["func$Local$app$lib$fn$decode_order$"],
   "solc": {"viair": ["Decoder.decodeOrder"], "legacy": ["Decoder.decodeOrder"]},
   "confidence": "high"}
]
```

Fe bytes are the primary-source bytes of the named bodies (from the stage
report's expansion by body); solc bytes are the named functions' bytes in each
build's `solc-functions` report. Inlined code counts toward the function it
came from on both sides. Every name must be in its report and every build
must be given, or the command refuses. With `--fe-total`, a residual row holds
everything outside the pairs, so each column adds up to its artifact; it is
negative when pairs overlap. Report schema `riffcat-function-comparison/1`.
