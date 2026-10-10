#!/usr/bin/env python3
"""A producer written outside Rust: arithmetic expressions as riff-catalog units.

It writes a recipe declaration (`calc-1.json`) and one unit file per world
(`left.jsonl`, `right.jsonl`), using nothing but the JSON format. `run.sh`
hands them to `riffcat-core`.

The recipe `calc/1` represents one expression as one graph:

- `calc.bin`: a binary operation. Field `op` (structure). Children `lhs` and
  `rhs`, one each.
- `calc.var`: a variable. Field `name` (names).
- `calc.num`: a literal. Field `value` (constants).
"""

import json
import pathlib
import sys

RECIPE = "calc/1"


def declaration():
    def field(name, dimension, value):
        return {"name": name, "dimensions": [dimension], "value": value, "occurs": "one"}

    operands = ["calc.bin", "calc.num", "calc.var"]
    return {
        "recipe": RECIPE,
        "roots": operands,
        "single_root": True,
        "kinds": [
            {"kind": "calc.bin", "fields": [field("op", "structure", "text")]},
            {"kind": "calc.var", "fields": [field("name", "names", "text")]},
            {"kind": "calc.num", "fields": [field("value", "constants", "u64")]},
        ],
        "children": [
            {"parents": ["calc.bin"], "label": "lhs", "children": operands, "shape": "one"},
            {"parents": ["calc.bin"], "label": "rhs", "children": operands, "shape": "one"},
        ],
        "edges": [],
        "acyclic": "children",
        "keys": "owner = the expression's name; local = the path from the root (lhs/rhs steps)",
    }


def unit(name, expression):
    """One unit. `expression` is a nested tuple: ("+", l, r), a str or an int."""
    owner = {"kind": "calc.expr", "owner": name, "local": "expr"}
    nodes, children = [], []

    def key(path):
        return {"entity": {"kind": "calc.node", "owner": name, "local": path}}

    def walk(expr, path):
        if isinstance(expr, tuple):
            op, lhs, rhs = expr
            kind = "calc.bin"
            fields = [{"dimension": "structure", "name": "op", "value": {"kind": "text", "value": op}}]
            for label, operand in (("lhs", lhs), ("rhs", rhs)):
                child = f"{path}/{label}"
                walk(operand, child)
                children.append({"parent": key(path), "label": label, "ordinal": 0, "child": key(child)})
        elif isinstance(expr, str):
            kind = "calc.var"
            fields = [{"dimension": "names", "name": "name", "value": {"kind": "text", "value": expr}}]
        else:
            kind = "calc.num"
            fields = [{"dimension": "constants", "name": "value", "value": {"kind": "u64", "value": expr}}]
        nodes.append([key(path), {"key": key(path), "kind": kind, "fields": fields}])

    walk(expression, "root")
    return {
        "recipe": RECIPE,
        "population": "calc-expr",
        "name": name,
        "graph": {
            "graph_key": {"owner": owner, "local": "expr"},
            "nodes": nodes,
            "children": children,
            "edges": [],
        },
    }


# Two worlds holding the same three expressions. `area` is written alike in
# both; `scale` differs in its variable names only; `offset` differs in a
# constant only.
WORLDS = {
    "left": {
        "area": ("*", "width", "height"),
        "scale": ("*", ("+", "x", 1), "factor"),
        "offset": ("+", "base", 32),
    },
    "right": {
        "area": ("*", "width", "height"),
        "scale": ("*", ("+", "input", 1), "k"),
        "offset": ("+", "base", 64),
    },
}


def main():
    out = pathlib.Path(sys.argv[1] if len(sys.argv) > 1 else ".")
    out.mkdir(parents=True, exist_ok=True)
    (out / "calc-1.json").write_text(json.dumps(declaration(), indent=1) + "\n")
    for world, expressions in WORLDS.items():
        lines = [json.dumps(unit(name, expr), separators=(",", ":")) for name, expr in expressions.items()]
        (out / f"{world}.jsonl").write_text("\n".join(lines) + "\n")


if __name__ == "__main__":
    main()
