//! The shared Yul AST. Field names and serde attributes match solc's
//! `irAst`/`irOptimizedAst` JSON exactly (verified against solc's irAst over
//! the vendored fixture contracts); the text parser constructs the same types.
//!
//! Normalizations baked into the type so both front doors agree (derived
//! `PartialEq` is conformance level 0):
//! - `src`/`nativeSrc` are not modeled (unknown JSON fields are ignored).
//! - String literal values are canonical `0x`-hex of their BYTES: the JSON
//!   path prefers solc's `hexValue` (escape-free), the parser unescapes;
//!   number spellings stay verbatim (canonicalized later, in `canon`).
//! - Empty `type` strings (untyped EVM dialect) become `None`, matching the
//!   parser's absent `:type` suffix.
//! - Data segments keep only their BYTES; solc's JSON drops data names
//!   (verified: YulData has no `name` key), so the parser drops them too.

use serde::{Deserialize, Deserializer, Serialize};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Object {
    pub name: String,
    pub code: Code,
    #[serde(default, rename = "subObjects")]
    pub sub_objects: Vec<ObjectChild>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "nodeType")]
pub enum ObjectChild {
    #[serde(rename = "YulObject")]
    Object(Object),
    #[serde(rename = "YulData")]
    Data(Data),
}

/// A `data` segment: canonical `0x`-hex of the bytes. No name — see module
/// docs.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Data {
    pub value: String,
}

impl<'de> Deserialize<'de> for Data {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        struct Raw {
            #[serde(default)]
            value: String,
        }
        let raw = Raw::deserialize(deserializer)?;
        Ok(Data {
            value: format!("0x{}", raw.value.to_lowercase()),
        })
    }
}

impl Data {
    pub fn from_bytes(bytes: &[u8]) -> Self {
        let mut value = String::with_capacity(2 + bytes.len() * 2);
        value.push_str("0x");
        for byte in bytes {
            value.push_str(&format!("{byte:02x}"));
        }
        Self { value }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Code {
    pub block: Block,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Block {
    #[serde(default)]
    pub statements: Vec<Statement>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "nodeType")]
pub enum Statement {
    #[serde(rename = "YulBlock")]
    Block(Block),
    #[serde(rename = "YulFunctionDefinition")]
    FunctionDefinition(FunctionDefinition),
    #[serde(rename = "YulVariableDeclaration")]
    VariableDeclaration(VariableDeclaration),
    #[serde(rename = "YulAssignment")]
    Assignment(Assignment),
    #[serde(rename = "YulExpressionStatement")]
    Expression(ExpressionStatement),
    #[serde(rename = "YulIf")]
    If(If),
    #[serde(rename = "YulSwitch")]
    Switch(Switch),
    #[serde(rename = "YulForLoop")]
    ForLoop(ForLoop),
    #[serde(rename = "YulBreak")]
    Break,
    #[serde(rename = "YulContinue")]
    Continue,
    #[serde(rename = "YulLeave")]
    Leave,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct FunctionDefinition {
    pub name: String,
    /// Key OMITTED by solc when empty (verified) — hence the defaults.
    #[serde(default)]
    pub parameters: Vec<TypedName>,
    #[serde(default, rename = "returnVariables")]
    pub return_variables: Vec<TypedName>,
    pub body: Block,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct TypedName {
    pub name: String,
    #[serde(
        default,
        rename = "type",
        deserialize_with = "empty_string_as_none",
        skip_serializing_if = "Option::is_none"
    )]
    pub ty: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct VariableDeclaration {
    pub variables: Vec<TypedName>,
    #[serde(default)]
    pub value: Option<Expression>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Assignment {
    #[serde(rename = "variableNames")]
    pub variable_names: Vec<Identifier>,
    pub value: Expression,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExpressionStatement {
    pub expression: Expression,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct If {
    pub condition: Expression,
    pub body: Block,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Switch {
    pub expression: Expression,
    pub cases: Vec<Case>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Case {
    /// `None` = the default case. solc's JSON encodes it as the literal
    /// string `"default"` in place of a literal object (verified).
    #[serde(deserialize_with = "case_value")]
    pub value: Option<Literal>,
    pub body: Block,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ForLoop {
    pub pre: Block,
    pub condition: Expression,
    pub post: Block,
    pub body: Block,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "nodeType")]
pub enum Expression {
    #[serde(rename = "YulFunctionCall")]
    FunctionCall(FunctionCall),
    #[serde(rename = "YulIdentifier")]
    Identifier(Identifier),
    #[serde(rename = "YulLiteral")]
    Literal(Literal),
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct FunctionCall {
    #[serde(rename = "functionName")]
    pub function_name: Identifier,
    pub arguments: Vec<Expression>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Identifier {
    pub name: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum LiteralKind {
    Number,
    String,
    Bool,
}

/// A literal. `value` is the verbatim spelling for numbers ("0x40" and "64"
/// are intentionally distinct here — `canon` unifies them at lowering),
/// canonical `0x`-hex of bytes for strings, "true"/"false" for bools.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Literal {
    pub kind: LiteralKind,
    pub value: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ty: Option<String>,
}

impl<'de> Deserialize<'de> for Literal {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        struct Raw {
            kind: LiteralKind,
            #[serde(default)]
            value: Option<String>,
            #[serde(default, rename = "hexValue")]
            hex_value: Option<String>,
            #[serde(default, rename = "type")]
            ty: Option<String>,
        }
        let raw = Raw::deserialize(deserializer)?;
        let value = match raw.kind {
            LiteralKind::Number | LiteralKind::Bool => raw.value.unwrap_or_default(),
            LiteralKind::String => match raw.hex_value {
                Some(hex) => format!("0x{}", hex.to_lowercase()),
                None => {
                    let bytes = raw.value.unwrap_or_default().into_bytes();
                    Literal::string_from_bytes(&bytes).value
                }
            },
        };
        Ok(Literal {
            kind: raw.kind,
            value,
            ty: raw.ty.filter(|ty| !ty.is_empty()),
        })
    }
}

impl Literal {
    pub fn number(spelling: impl Into<String>) -> Self {
        Self {
            kind: LiteralKind::Number,
            value: spelling.into(),
            ty: None,
        }
    }

    pub fn bool(value: bool) -> Self {
        Self {
            kind: LiteralKind::Bool,
            value: if value { "true" } else { "false" }.to_string(),
            ty: None,
        }
    }

    pub fn string_from_bytes(bytes: &[u8]) -> Self {
        let mut value = String::with_capacity(2 + bytes.len() * 2);
        value.push_str("0x");
        for byte in bytes {
            value.push_str(&format!("{byte:02x}"));
        }
        Self {
            kind: LiteralKind::String,
            value,
            ty: None,
        }
    }
}

fn empty_string_as_none<'de, D: Deserializer<'de>>(
    deserializer: D,
) -> Result<Option<String>, D::Error> {
    let value = Option::<String>::deserialize(deserializer)?;
    Ok(value.filter(|s| !s.is_empty()))
}

fn case_value<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Option<Literal>, D::Error> {
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum Raw {
        Text(String),
        Literal(Literal),
    }
    match Raw::deserialize(deserializer)? {
        Raw::Text(text) if text == "default" => Ok(None),
        Raw::Text(other) => Err(serde::de::Error::custom(format!(
            "unexpected case value string {other:?}"
        ))),
        Raw::Literal(literal) => Ok(Some(literal)),
    }
}
