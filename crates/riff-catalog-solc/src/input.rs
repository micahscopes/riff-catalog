//! Standard-JSON input construction.

use std::collections::BTreeMap;

use serde_json::{Value, json};

/// Which Solidity codegen pipeline to use.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Pipeline {
    /// Classic codegen (Solidity -> EVM assembly). No IR outputs.
    Legacy,
    /// IR pipeline (Solidity -> Yul -> EVM); enables ir/irAst/irOptimized/
    /// irOptimizedAst/yulCFGJson outputs.
    ViaIr,
}

impl Pipeline {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Legacy => "legacy",
            Self::ViaIr => "via-ir",
        }
    }
}

#[derive(Clone, Debug)]
pub struct CompileOptions {
    pub pipeline: Pipeline,
    pub optimize: bool,
    pub runs: u32,
    /// None = solc default (sourcify settings may override per contract).
    pub evm_version: Option<String>,
}

impl Default for CompileOptions {
    fn default() -> Self {
        Self {
            pipeline: Pipeline::ViaIr,
            optimize: false,
            runs: 200,
            evm_version: None,
        }
    }
}

fn contract_outputs(pipeline: Pipeline) -> Vec<&'static str> {
    let mut outputs = vec!["evm.bytecode.object", "evm.deployedBytecode.object"];
    if pipeline == Pipeline::ViaIr {
        outputs.extend(["ir", "irAst", "irOptimized", "irOptimizedAst", "yulCFGJson"]);
    }
    outputs
}

/// Standard-JSON for Solidity sources. Requests per-file `ast` plus the
/// per-contract outputs for the chosen pipeline.
pub fn solidity_input(sources: &BTreeMap<String, String>, opts: &CompileOptions) -> Value {
    let sources_json: Value = sources
        .iter()
        .map(|(name, content)| (name.clone(), json!({ "content": content })))
        .collect::<serde_json::Map<String, Value>>()
        .into();

    let mut settings = json!({
        "optimizer": { "enabled": opts.optimize, "runs": opts.runs },
        "outputSelection": {
            "*": {
                "": ["ast"],
                "*": contract_outputs(opts.pipeline),
            }
        }
    });
    if opts.pipeline == Pipeline::ViaIr {
        settings["viaIR"] = json!(true);
    }
    if let Some(evm_version) = &opts.evm_version {
        settings["evmVersion"] = json!(evm_version);
    }

    json!({
        "language": "Solidity",
        "sources": sources_json,
        "settings": settings,
    })
}

/// Standard-JSON for direct Yul input. solc's assembler mode has no AST JSON
/// output (verified), but `yulCFGJson` IS available — that is the SSA-level
/// ingestion path for raw .yul (e.g. fe-emitted) sources.
pub fn yul_input(source_name: &str, yul: &str, optimize: bool) -> Value {
    json!({
        "language": "Yul",
        "sources": { source_name: { "content": yul } },
        "settings": {
            "optimizer": { "enabled": optimize },
            "outputSelection": {
                "*": {
                    "*": [
                        "evm.bytecode.object",
                        "evm.deployedBytecode.object",
                        "yulCFGJson",
                    ]
                }
            }
        }
    })
}

/// Take a verbatim standard-JSON input (e.g. sourcify's stdJsonInput) and
/// force our outputSelection, leaving every other setting untouched.
pub fn with_output_selection(mut std_json: Value, pipeline: Pipeline) -> Value {
    let selection = json!({
        "*": {
            "": ["ast"],
            "*": contract_outputs(pipeline),
        }
    });
    if std_json.get("settings").is_none() {
        std_json["settings"] = json!({});
    }
    std_json["settings"]["outputSelection"] = selection;
    if pipeline == Pipeline::ViaIr {
        std_json["settings"]["viaIR"] = json!(true);
    }
    std_json
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn solidity_input_shapes() {
        let sources: BTreeMap<String, String> =
            [("a.sol".to_string(), "contract A {}".to_string())].into();
        let legacy = solidity_input(
            &sources,
            &CompileOptions {
                pipeline: Pipeline::Legacy,
                ..Default::default()
            },
        );
        assert!(legacy["settings"].get("viaIR").is_none());
        let outputs = legacy["settings"]["outputSelection"]["*"]["*"]
            .as_array()
            .unwrap();
        assert!(!outputs.iter().any(|o| o == "irAst"));

        let viair = solidity_input(&sources, &CompileOptions::default());
        assert_eq!(viair["settings"]["viaIR"], json!(true));
        let outputs = viair["settings"]["outputSelection"]["*"]["*"]
            .as_array()
            .unwrap();
        for expected in ["ir", "irAst", "irOptimizedAst", "yulCFGJson"] {
            assert!(outputs.iter().any(|o| o == expected), "{expected}");
        }
    }

    #[test]
    fn yul_input_requests_ssa_cfg() {
        let input = yul_input("in.yul", "object \"D\" { code {} }", false);
        assert_eq!(input["language"], "Yul");
        let outputs = input["settings"]["outputSelection"]["*"]["*"]
            .as_array()
            .unwrap();
        assert!(outputs.iter().any(|o| o == "yulCFGJson"));
    }

    #[test]
    fn output_selection_override_preserves_settings() {
        let original = json!({
            "language": "Solidity",
            "sources": {},
            "settings": { "optimizer": { "enabled": true, "runs": 999 } }
        });
        let forced = with_output_selection(original, Pipeline::ViaIr);
        assert_eq!(forced["settings"]["optimizer"]["runs"], json!(999));
        assert_eq!(forced["settings"]["viaIR"], json!(true));
        assert!(forced["settings"]["outputSelection"]["*"]["*"].is_array());
    }
}
