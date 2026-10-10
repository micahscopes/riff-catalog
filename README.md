# riff-catalog

Finds graphs that are the same once you ignore the parts you choose to ignore.

Two functions that differ only in their variable names get the same address when names are ignored. That is
enough to find renamed or repeated code, or to compare what two compilers make of one program.

This repository is the engine and a command-line tool. It knows no programming language.

**Experimental.** Formats and addresses change without notice.

## Try it

Needs Rust and Python 3.

```sh
cargo install --locked --path crates/riff-catalog-tool
examples/outside-producer/run.sh
```

The script writes six small arithmetic expressions as graphs, then asks three questions: which are identical,
which are identical apart from variable names, and which have the same structure.

## How it works

1. You describe your data as graphs: nodes with a kind and some fields, children, and edges.
2. Every field goes into one of five bins: structure, names, constants, types, trace events.
3. Each graph is hashed once per bin.
4. A question picks bins, for example "everything except names". The answer is put together from the stored
   hashes. Nothing is hashed again.

## Words

| Word | Meaning |
|---|---|
| recipe | a named, versioned way to turn something into graphs, such as `calc/1` |
| declaration | a JSON file listing what a recipe's graphs may contain; every graph is checked against it |
| unit | one graph, with its recipe, what kind of thing it is, and a display name |
| world | where a batch of units came from: a compiler and commit, a model, a run |
| facet | the bins a question keeps; `names-blind` keeps all but names |
| bound or free keys | whether node identities must match too (bound), or only the shape they form (free) |

## What an equal address tells you

- The two graphs are the same after dropping everything outside the facet.
- It is about the graphs. It says nothing about behavior unless the recipe was built to make it so.
- A few very regular cyclic graphs can get equal addresses although they differ. `riff-catalog-exact` compares
  small graphs exactly when that matters.
- Hash collisions are assumed not to happen (BLAKE3).

## Use it with your own data

Write units as JSON, one per line, and a declaration as a JSON file. Any language will do;
[`examples/outside-producer/producer.py`](examples/outside-producer/producer.py) is about 100 lines.

```sh
riffcat-core check --declaration calc-1.json left.jsonl right.jsonl    # do the files fit the declaration?
riffcat-core add --declaration calc-1.json --world left left.jsonl     # store them
riffcat-core add --world right right.jsonl
riffcat-core bucket --unit calc-expr --facet names-blind               # group what is the same
```

| Command | What it does |
|---|---|
| `check` | checks unit files against their declaration; stores nothing |
| `add` | stores unit files under a world; adding a file again replaces what it stored before |
| `bucket` | groups units that have the same address at a facet |
| `overlap` | lists the addresses two selections share |
| `diff` | compares two selections bin by bin |
| `root` | one hash for everything stored, whatever the order it was added in |
| `recipes`, `declaration` | lists recipes; prints a declaration, or drafts one from example units |
| `view` | applies a saved graph transform and stores the results |
| `claim`, `attest` | records that two addresses are equivalent, or that one has a property, with the evidence |

Everything is stored as JSON-lines files in one directory (`--corpus`, default `./corpus`).

## Use it from Rust

Depend on `riff-catalog-tool`, declare your recipes to a `Corpus`, store units with `admit_units`, and add the
commands above to your own tool with `#[command(flatten)]`.

| Crate | Contents |
|---|---|
| `riff-catalog-schema` | graphs, units, declarations and their JSON form |
| `riff-catalog-core` | hashing, facets, addresses |
| `riff-catalog-exact` | exact comparison of small graphs |
| `riff-catalog-claims` | equivalence claims and property attestations |
| `riff-catalog-view` | saved graph transforms |
| `riff-catalog-tool` | storage, the commands, the `riffcat-core` program |

## Checked twice

The hashing is written a second time in Lean, and both versions must produce the same bytes on a set of test
graphs. See [`lean/`](lean/README.md).

```sh
cargo test --workspace
(cd lean && lake exe check)
```

## Where the language support is

Recipes and tools for Solidity, Yul, EVM bytecode, Fe and others live in a separate repository,
riff-catalog-lab, which is not public yet.
