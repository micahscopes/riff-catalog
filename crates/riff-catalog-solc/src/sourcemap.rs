//! solc source maps joined to the Solidity AST: which source function each
//! emitted instruction belongs to.
//!
//! A source map has one entry per instruction, `s:l:f:j:m` separated by `;`,
//! where an empty field repeats the previous entry's value. `f` is a source
//! id (`sources.<name>.id` in the standard-JSON output); `-1` means no
//! source, and ids past the Solidity sources name compiler-generated Yul
//! (`generatedSources`). An instruction belongs to the innermost
//! `FunctionDefinition` or `ModifierDefinition` whose source range contains
//! its range in the same file. solc keeps an inlined callee's source range,
//! so inlined code counts toward the callee, not the caller.

use serde_json::Value;

use crate::output::SolcOutput;

/// One decoded source map entry.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SourceMapEntry {
    pub start: i64,
    pub length: i64,
    pub file: i64,
    pub jump: char,
    pub modifier_depth: i64,
}

/// A source map field that does not parse.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
#[error("source map entry {entry}: {message}")]
pub struct SourceMapError {
    pub entry: usize,
    pub message: String,
}

/// Decode a compressed source map. An empty field repeats the previous
/// entry's value; a field that does not parse is an error, never silently
/// replaced by the previous value.
pub fn parse_source_map(map: &str) -> Result<Vec<SourceMapEntry>, SourceMapError> {
    let mut out = Vec::new();
    let mut prev = SourceMapEntry {
        start: 0,
        length: 0,
        file: -1,
        jump: '-',
        modifier_depth: 0,
    };
    if map.is_empty() {
        return Ok(out);
    }
    for (entry, item) in map.split(';').enumerate() {
        let mut cur = prev;
        let error = |message: String| SourceMapError { entry, message };
        let number = |field: &str, what: &str| {
            field
                .parse::<i64>()
                .map_err(|_| error(format!("{what} `{field}` is not an integer")))
        };
        for (i, field) in item.split(':').enumerate() {
            if field.is_empty() {
                continue;
            }
            match i {
                0 => cur.start = number(field, "start")?,
                1 => cur.length = number(field, "length")?,
                2 => cur.file = number(field, "source id")?,
                3 => {
                    cur.jump = match field {
                        "i" | "o" | "-" => field.chars().next().expect("non-empty"),
                        _ => return Err(error(format!("jump `{field}` is not i, o or -"))),
                    }
                }
                4 => cur.modifier_depth = number(field, "modifier depth")?,
                _ => return Err(error(format!("more than five fields in `{item}`"))),
            }
        }
        out.push(cur);
        prev = cur;
    }
    Ok(out)
}

/// A function or modifier definition's source range.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FunctionSpan {
    pub file: i64,
    pub start: i64,
    pub end: i64,
    /// `Contract.function`, or `<file>::(free).function` outside contracts.
    pub name: String,
}

/// Where one instruction's source map entry points.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SourceOwner {
    /// Index into the spans returned by [`function_spans`].
    Function(usize),
    /// Inside a contract but no function (for example the dispatcher).
    Contract(String),
    /// A Solidity file, outside any contract.
    File(i64),
    /// Compiler-generated Yul (an id past the Solidity sources).
    Generated,
    /// `f = -1`.
    NoSource,
}

fn src_range(node: &Value) -> Option<(i64, i64, i64)> {
    let mut parts = node.get("src")?.as_str()?.split(':');
    let start: i64 = parts.next()?.parse().ok()?;
    let len: i64 = parts.next()?.parse().ok()?;
    let file: i64 = parts.next()?.parse().ok()?;
    Some((start, start + len, file))
}

/// Every function and modifier definition, and every contract range, in the
/// output's Solidity ASTs.
pub fn function_spans(output: &SolcOutput) -> (Vec<FunctionSpan>, Vec<FunctionSpan>) {
    let mut functions = Vec::new();
    let mut contracts = Vec::new();
    let Some(sources) = output.raw().get("sources").and_then(Value::as_object) else {
        return (functions, contracts);
    };
    for (name, source) in sources {
        let Some(ast) = source.get("ast") else {
            continue;
        };
        let base = name.rsplit('/').next().unwrap_or(name).to_string();
        let mut stack: Vec<(&Value, Option<String>)> = vec![(ast, None)];
        while let Some((node, contract)) = stack.pop() {
            let mut contract = contract;
            if let Some(obj) = node.as_object() {
                match obj.get("nodeType").and_then(Value::as_str) {
                    Some("ContractDefinition") => {
                        let cname = obj.get("name").and_then(Value::as_str).unwrap_or("?");
                        if let Some((s, e, f)) = src_range(node) {
                            contracts.push(FunctionSpan {
                                file: f,
                                start: s,
                                end: e,
                                name: cname.to_string(),
                            });
                        }
                        contract = Some(cname.to_string());
                    }
                    Some("FunctionDefinition" | "ModifierDefinition") => {
                        let mut fname = obj
                            .get("name")
                            .and_then(Value::as_str)
                            .unwrap_or("")
                            .to_string();
                        if fname.is_empty() {
                            fname = obj
                                .get("kind")
                                .and_then(Value::as_str)
                                .unwrap_or("fn")
                                .to_string();
                        }
                        let owner = contract
                            .clone()
                            .unwrap_or_else(|| format!("{base}::(free)"));
                        if let Some((s, e, f)) = src_range(node) {
                            functions.push(FunctionSpan {
                                file: f,
                                start: s,
                                end: e,
                                name: format!("{owner}.{fname}"),
                            });
                        }
                    }
                    _ => {}
                }
                for value in obj.values() {
                    if value.is_object() || value.is_array() {
                        stack.push((value, contract.clone()));
                    }
                }
            } else if let Some(items) = node.as_array() {
                for item in items {
                    stack.push((item, contract.clone()));
                }
            }
        }
    }
    (functions, contracts)
}

/// The owner of every source map entry: the innermost function span that
/// contains it, else the innermost contract, else its file.
pub fn attribute(
    entries: &[SourceMapEntry],
    functions: &[FunctionSpan],
    contracts: &[FunctionSpan],
    solidity_ids: &[i64],
) -> Vec<SourceOwner> {
    let innermost = |spans: &[FunctionSpan], e: &SourceMapEntry| -> Option<usize> {
        spans
            .iter()
            .enumerate()
            .filter(|(_, s)| s.file == e.file && s.start <= e.start && e.start + e.length <= s.end)
            .min_by_key(|(_, s)| s.end - s.start)
            .map(|(i, _)| i)
    };
    entries
        .iter()
        .map(|e| {
            if e.file < 0 {
                SourceOwner::NoSource
            } else if !solidity_ids.contains(&e.file) {
                innermost(functions, e).map_or(SourceOwner::Generated, SourceOwner::Function)
            } else if let Some(i) = innermost(functions, e) {
                SourceOwner::Function(i)
            } else if let Some(i) = innermost(contracts, e) {
                SourceOwner::Contract(contracts[i].name.clone())
            } else {
                SourceOwner::File(e.file)
            }
        })
        .collect()
}

/// Function definitions in compiler-generated Yul (`generatedSources` of a
/// bytecode object), named `(yul) <name>`, so generated helpers such as ABI
/// coders and checked arithmetic get their own owners.
pub fn generated_function_spans(bytecode_object: &Value) -> Vec<FunctionSpan> {
    let mut out = Vec::new();
    let Some(sources) = bytecode_object
        .get("generatedSources")
        .and_then(Value::as_array)
    else {
        return out;
    };
    for source in sources {
        let mut stack: Vec<&Value> = source.get("ast").into_iter().collect();
        while let Some(node) = stack.pop() {
            if let Some(obj) = node.as_object() {
                if obj.get("nodeType").and_then(Value::as_str) == Some("YulFunctionDefinition")
                    && let (Some(name), Some((s, e, f))) =
                        (obj.get("name").and_then(Value::as_str), src_range(node))
                {
                    out.push(FunctionSpan {
                        file: f,
                        start: s,
                        end: e,
                        name: format!("(yul) {name}"),
                    });
                }
                stack.extend(obj.values().filter(|v| v.is_object() || v.is_array()));
            } else if let Some(items) = node.as_array() {
                stack.extend(items.iter());
            }
        }
    }
    out
}

/// Source ids of the output's Solidity sources.
pub fn solidity_source_ids(output: &SolcOutput) -> Vec<i64> {
    output
        .raw()
        .get("sources")
        .and_then(Value::as_object)
        .map(|s| {
            s.values()
                .filter_map(|v| v.get("id").and_then(Value::as_i64))
                .collect()
        })
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn source_maps_repeat_empty_fields() {
        let e = parse_source_map("1:2:0:-:0;;3::1;:4:-1:i").unwrap();
        assert_eq!(e.len(), 4);
        assert_eq!((e[1].start, e[1].length, e[1].file), (1, 2, 0));
        assert_eq!((e[2].start, e[2].length, e[2].file), (3, 2, 1));
        assert_eq!((e[3].length, e[3].file, e[3].jump), (4, -1, 'i'));
    }

    #[test]
    fn malformed_fields_are_errors() {
        for bad in [
            "12:3:0;zz:q:0",
            "1:2:x",
            "1:2:0:j",
            "1:2:0:-:y",
            "1:2:0:-:0:9",
        ] {
            assert!(parse_source_map(bad).is_err(), "{bad} accepted");
        }
    }

    #[test]
    fn innermost_function_owns_an_entry() {
        let raw = serde_json::json!({"sources": {"a.sol": {"id": 0, "ast": {
        "nodeType": "SourceUnit", "src": "0:100:0", "nodes": [
            {"nodeType": "ContractDefinition", "name": "C", "src": "0:100:0", "nodes": [
                {"nodeType": "FunctionDefinition", "name": "f", "src": "10:50:0"},
                {"nodeType": "FunctionDefinition", "name": "g", "src": "20:10:0"}
            ]}
        ]}}}});
        let out = SolcOutput::new(raw);
        let (functions, contracts) = function_spans(&out);
        let entries = parse_source_map("22:3:0;12:3:0;70:1:0;5:1:7;1:1:-1").unwrap();
        let owners = attribute(&entries, &functions, &contracts, &solidity_source_ids(&out));
        let name = |o: &SourceOwner| match o {
            SourceOwner::Function(i) => functions[*i].name.clone(),
            other => format!("{other:?}"),
        };
        assert_eq!(name(&owners[0]), "C.g");
        assert_eq!(name(&owners[1]), "C.f");
        assert_eq!(owners[2], SourceOwner::Contract("C".into()));
        assert_eq!(owners[3], SourceOwner::Generated);
        let generated = serde_json::json!({"generatedSources": [{"id": 7, "ast": {
        "nodeType": "YulBlock", "src": "0:50:7", "statements": [
            {"nodeType": "YulFunctionDefinition", "name": "abi_decode_x", "src": "2:10:7"}
        ]}}]});
        let mut all = functions.clone();
        all.extend(generated_function_spans(&generated));
        let owners = attribute(&entries, &all, &contracts, &solidity_source_ids(&out));
        assert!(
            matches!(owners[3], SourceOwner::Function(i) if all[i].name == "(yul) abi_decode_x")
        );
        assert_eq!(owners[4], SourceOwner::NoSource);
    }
}
