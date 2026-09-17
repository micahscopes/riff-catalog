//! Experimental batch protocol over explicit ordered pure regions.
//! No source parsing, CFG canonicalization, or bug classification is implied.
use anyhow::{Result, ensure};
use serde::{Deserialize, Serialize};

use crate::{NormalOperand, NormalRegion, Region};

pub const REQUEST_SCHEMA: &str = "riffcat-ordered-region-request/1";
pub const RESPONSE_SCHEMA: &str = "riffcat-ordered-region-response/1";
pub const MAX_OPERATIONS: usize = 4096;

#[derive(Clone, Copy, Debug, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum View {
    /// Literal strings are compared exactly; alternate numeric spellings are
    /// not normalized. Compiler adapters may normalize before submission.
    LiteralTokens,
    IgnoreLiterals,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Request {
    pub schema: String,
    pub id: String,
    pub view: View,
    pub left: Region,
    pub right: Region,
}

#[derive(Debug, Serialize)]
pub struct Response {
    pub schema: &'static str,
    pub id: String,
    pub view: View,
    pub status: &'static str,
    pub equivalent: bool,
    pub left: NormalRegion,
    pub right: NormalRegion,
    pub left_bindings: Vec<String>,
    pub right_bindings: Vec<String>,
}

fn project(region: &Region, view: View) -> Result<(NormalRegion, Vec<String>)> {
    ensure!(
        region.operations.len() <= MAX_OPERATIONS,
        "region operation limit exceeded"
    );
    for operation in &region.operations {
        for operand in &operation.operands {
            if let crate::Operand::External(name) = operand {
                ensure!(!name.is_empty(), "external binding key must not be empty");
            }
            if let crate::Operand::Literal(value) = operand {
                ensure!(!value.is_empty(), "literal token must not be empty");
            }
        }
    }
    let (mut normal, bindings) = region.normalize()?;
    if matches!(view, View::IgnoreLiterals) {
        for operation in &mut normal.operations {
            for operand in &mut operation.operands {
                if let NormalOperand::Literal(value) = operand {
                    *value = "<literal>".into();
                }
            }
        }
    }
    Ok((normal, bindings))
}

pub fn compare(request: Request) -> Result<Response> {
    ensure!(
        request.schema == REQUEST_SCHEMA,
        "unsupported request schema"
    );
    ensure!(!request.id.is_empty(), "request id must not be empty");
    let (left, left_bindings) = project(&request.left, request.view)?;
    let (right, right_bindings) = project(&request.right, request.view)?;
    // This compares the complete normalized records jointly. It does not use
    // equality of historical per-dimension hashes as an exactness certificate.
    Ok(Response {
        schema: RESPONSE_SCHEMA,
        id: request.id,
        view: request.view,
        status: "compared",
        equivalent: left == right,
        left,
        right,
        left_bindings,
        right_bindings,
    })
}
