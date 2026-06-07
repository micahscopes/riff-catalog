//! Thin typed accessors over the raw standard-JSON output. Deliberately NOT
//! full serde structs: the Solidity AST schema is huge and version-volatile;
//! consumers pick out exactly what they lower.

use serde_json::Value;

use crate::error::{SolcDiagnostic, SolcError};

pub struct SolcOutput {
    raw: Value,
}

impl SolcOutput {
    pub fn new(raw: Value) -> Self {
        Self { raw }
    }

    pub fn raw(&self) -> &Value {
        &self.raw
    }

    /// Error if any diagnostic has severity "error"; warnings pass through.
    pub fn check_errors(&self) -> Result<(), SolcError> {
        let Some(diagnostics) = self.raw.get("errors").and_then(Value::as_array) else {
            return Ok(());
        };
        let errors: Vec<SolcDiagnostic> = diagnostics
            .iter()
            .filter_map(|d| serde_json::from_value::<SolcDiagnostic>(d.clone()).ok())
            .filter(|d| d.severity == "error")
            .collect();
        if errors.is_empty() {
            Ok(())
        } else {
            Err(SolcError::Diagnostics(errors))
        }
    }

    /// Per-file Solidity AST (`sources.<name>.ast`).
    pub fn source_ast(&self, source: &str) -> Result<&Value, SolcError> {
        self.raw
            .pointer(&format!("/sources/{}/ast", escape(source)))
            .ok_or_else(|| SolcError::MissingOutput(format!("sources.{source}.ast")))
    }

    /// All (source, contract) pairs present in `contracts`.
    pub fn contract_names(&self) -> Vec<(String, String)> {
        let mut names = Vec::new();
        if let Some(contracts) = self.raw.get("contracts").and_then(Value::as_object) {
            for (source, inner) in contracts {
                if let Some(inner) = inner.as_object() {
                    for contract in inner.keys() {
                        names.push((source.clone(), contract.clone()));
                    }
                }
            }
        }
        names
    }

    fn contract_field(
        &self,
        source: &str,
        contract: &str,
        field: &str,
    ) -> Result<&Value, SolcError> {
        self.raw
            .pointer(&format!(
                "/contracts/{}/{}/{}",
                escape(source),
                escape(contract),
                escape(field)
            ))
            .ok_or_else(|| {
                SolcError::MissingOutput(format!("contracts.{source}.{contract}.{field}"))
            })
    }

    pub fn ir(&self, source: &str, contract: &str) -> Result<&str, SolcError> {
        self.contract_field(source, contract, "ir")?
            .as_str()
            .ok_or_else(|| SolcError::MissingOutput("ir is not a string".into()))
    }

    pub fn ir_optimized(&self, source: &str, contract: &str) -> Result<&str, SolcError> {
        self.contract_field(source, contract, "irOptimized")?
            .as_str()
            .ok_or_else(|| SolcError::MissingOutput("irOptimized is not a string".into()))
    }

    pub fn ir_ast(&self, source: &str, contract: &str) -> Result<&Value, SolcError> {
        self.contract_field(source, contract, "irAst")
    }

    pub fn ir_optimized_ast(&self, source: &str, contract: &str) -> Result<&Value, SolcError> {
        self.contract_field(source, contract, "irOptimizedAst")
    }

    /// The experimental SSA CFG (Moritz's work). Available for Solidity
    /// viaIR AND direct Yul input.
    pub fn yul_cfg_json(&self, source: &str, contract: &str) -> Result<&Value, SolcError> {
        self.contract_field(source, contract, "yulCFGJson")
    }

    pub fn bytecode(&self, source: &str, contract: &str) -> Result<Vec<u8>, SolcError> {
        let object = self
            .contract_field(source, contract, "evm")?
            .pointer("/bytecode/object")
            .and_then(Value::as_str)
            .ok_or_else(|| SolcError::MissingOutput("evm.bytecode.object".into()))?;
        hex::decode(object).map_err(|e| SolcError::BytecodeHex(e.to_string()))
    }

    pub fn deployed_bytecode(&self, source: &str, contract: &str) -> Result<Vec<u8>, SolcError> {
        let object = self
            .contract_field(source, contract, "evm")?
            .pointer("/deployedBytecode/object")
            .and_then(Value::as_str)
            .ok_or_else(|| SolcError::MissingOutput("evm.deployedBytecode.object".into()))?;
        hex::decode(object).map_err(|e| SolcError::BytecodeHex(e.to_string()))
    }
}

/// JSON-pointer escaping per RFC 6901.
fn escape(segment: &str) -> String {
    segment.replace('~', "~0").replace('/', "~1")
}
