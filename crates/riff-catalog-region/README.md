# Explicit ordered regions

Recovered from the isolated 2026-09-04 region-boundary pilot. This is the first
implementation slice for code-as-query, not a finished source search engine.

The library preserves ordered pure operations, backward result references,
abstract external inputs and explicit exported result slots. Input ports are
numbered by first use; occurrence bindings are returned separately. Supported
operations are add, mul, xor, sub, and, or, each with two operands and one result.
No CFG cycles, side effects, automatic region discovery or semantic equivalence
are supported by this particular contract.

## Batch CLI

`riffcat region --jsonl` reads one request per stdin line and emits one response
per stdout line. No corpus is opened and no compiler is started. This protocol
is experimental and intentionally specific, not the eventual source-search API.

```json
{"schema":"riffcat-ordered-region-request/1","id":"rename","view":"literal-tokens","left":{"operations":[{"op":"add","operands":[{"External":"x"},{"Literal":"7"}]}],"outputs":[0]},"right":{"operations":[{"op":"add","operands":[{"External":"renamed"},{"Literal":"7"}]}],"outputs":[0]}}
```

The response has schema `riffcat-ordered-region-response/1`, the request ID/view,
status `compared`, an `equivalent` boolean, both projected normalized records and
their external binding lists. Equality compares these complete records, not hashes.
Input keys are arbitrary resolved-entity identifiers, not necessarily source names.
Callers must not pretend unresolved source spellings are resolved declarations.

`literal-tokens` preserves literal strings verbatim: `7` and `0x7` differ.
`ignore-literals` retains literal positions but replaces their contents. Neither
policy establishes numeric evaluation or behavior. Export slots must be unique,
valid and in definition order; changing the interface can change equality.

Unknown fields, schemas, unsupported operations and invalid references are errors,
never ordinary nonmatches. Errors have status `error`, physical input line number,
message and ID when a complete request could be decoded. Blank lines are invalid.
Limits: 1 MiB per input line including newline, 4096 operations per region.
Oversized records are drained without retaining the entire line.

Batch processing continues after record errors. Exit status is 0 if every request
succeeds (including valid unequal comparisons), 1 for any record/IO error; clap
argument errors use its standard status 2. Diagnostics go to stderr. Output is
flushed per request, so callers can stream and cancel by ending the process.

## Reproduction

```sh
cargo test -p riff-catalog-region
RIFFCAT_SOLC=/workspace/build/solidity-make/solc/solc cargo run -p riff-catalog-region --example reproduce -- /workspace/region-reproduction
```

Configure TMPDIR, target and sccache paths under /workspace before building in
this sandbox. Compiler reproduction needs experimental `yulCFGJson`; offline
contract tests do not. `from_yul_graph` is still the explicitly documented
historical add/mul/xor selector. It is not exposed as generic source search.

The exhaustive port-renaming oracle, alias-incidence regression and WL examples
were recovered with the implementation. The WL examples intentionally show that
fingerprint equality is weaker than exact graph isomorphism. Their public module
is experimental, not the proposed general canonicalization API.
