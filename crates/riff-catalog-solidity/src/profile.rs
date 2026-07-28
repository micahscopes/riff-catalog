//! The declarative Solidity profile: per nodeType, what the walker reads.
//! Everything not named here is never read — volatile fields (`id`, `src`,
//! `nameLocation`, selectors, `isSimpleCounterLoop`, …) drop out by
//! construction.
//!
//! Covers the nodeType inventory of the vendored fixtures plus rows certain to
//! appear in real-world (sourcify) contracts. Yul nodes appear too: InlineAssembly
//! embeds a YulBlock under its `AST` key, walked by the same machinery.

/// Where a node starts a new graph unit.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Unit {
    None,
    /// One `sol-contract` graph per ContractDefinition.
    Contract,
    /// One `sol-fn` graph per Function/ModifierDefinition.
    Function,
}

/// How to pull a Types-dimension value out of the node.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TypeExtract {
    /// `typeDescriptions.typeString`
    TypeString,
    /// a plain string field, e.g. ElementaryTypeName's `name`
    Field(&'static str),
}

/// How to pull Constants-dimension values out of the node.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ConstExtract {
    /// Solidity Literal: canonicalize via kind/value/hexValue.
    SolLiteral,
    /// PragmaDirective: join the `literals` array with spaces.
    PragmaLiterals,
    /// Yul literal embedded in inline assembly (kind/value/hexValue).
    YulLiteral,
}

#[derive(Clone, Copy, Debug)]
pub struct NodeSpec {
    pub kind: &'static str,
    /// (json field, child label), declaration order = ordinal order. A field
    /// holding an array contributes one child per element; null/missing
    /// fields contribute nothing (structurally absent == JSON null).
    pub children: &'static [(&'static str, &'static str)],
    /// String fields -> Names dimension.
    pub names: &'static [&'static str],
    /// Scalar fields -> Structure dimension (stringified).
    pub structure: &'static [&'static str],
    pub constants: &'static [ConstExtract],
    pub types: &'static [TypeExtract],
    pub unit: Unit,
}

impl NodeSpec {
    pub const EMPTY: NodeSpec = NodeSpec {
        kind: "",
        children: &[],
        names: &[],
        structure: &[],
        constants: &[],
        types: &[],
        unit: Unit::None,
    };
}

const TS: &[TypeExtract] = &[TypeExtract::TypeString];

pub fn spec_for(node_type: &str) -> Option<NodeSpec> {
    let spec = match node_type {
        // ----------------------------------------------------------- units
        "SourceUnit" => NodeSpec {
            kind: "sol.source-unit",
            children: &[("nodes", "decl")],
            ..NodeSpec::EMPTY
        },
        "ContractDefinition" => NodeSpec {
            kind: "sol.contract",
            children: &[("baseContracts", "base"), ("nodes", "member")],
            names: &["name"],
            structure: &["contractKind", "abstract"],
            unit: Unit::Contract,
            ..NodeSpec::EMPTY
        },
        "FunctionDefinition" => NodeSpec {
            kind: "sol.function",
            children: &[
                ("parameters", "params"),
                ("returnParameters", "returns"),
                ("modifiers", "modifier"),
                ("overrides", "override"),
                ("body", "body"),
            ],
            names: &["name"],
            structure: &["kind", "stateMutability", "visibility", "virtual"],
            unit: Unit::Function,
            ..NodeSpec::EMPTY
        },
        "ModifierDefinition" => NodeSpec {
            kind: "sol.modifier",
            children: &[("parameters", "params"), ("body", "body")],
            names: &["name"],
            structure: &["visibility", "virtual"],
            unit: Unit::Function,
            ..NodeSpec::EMPTY
        },
        // ----------------------------------------------------- declarations
        "PragmaDirective" => NodeSpec {
            kind: "sol.pragma",
            constants: &[ConstExtract::PragmaLiterals],
            ..NodeSpec::EMPTY
        },
        "ImportDirective" => NodeSpec {
            kind: "sol.import",
            names: &["unitAlias", "file"],
            ..NodeSpec::EMPTY
        },
        "UsingForDirective" => NodeSpec {
            kind: "sol.using",
            children: &[("libraryName", "lib"), ("typeName", "type")],
            structure: &["global"],
            ..NodeSpec::EMPTY
        },
        "InheritanceSpecifier" => NodeSpec {
            kind: "sol.inherit",
            children: &[("baseName", "name"), ("arguments", "arg")],
            ..NodeSpec::EMPTY
        },
        "ModifierInvocation" => NodeSpec {
            kind: "sol.modifier-call",
            children: &[("modifierName", "name"), ("arguments", "arg")],
            ..NodeSpec::EMPTY
        },
        "ParameterList" => NodeSpec {
            kind: "sol.params",
            children: &[("parameters", "param")],
            ..NodeSpec::EMPTY
        },
        "VariableDeclaration" => NodeSpec {
            kind: "sol.var",
            children: &[("typeName", "type"), ("value", "init")],
            names: &["name"],
            structure: &[
                "mutability",
                "stateVariable",
                "storageLocation",
                "visibility",
                "indexed",
                "constant",
            ],
            types: TS,
            ..NodeSpec::EMPTY
        },
        "StructDefinition" => NodeSpec {
            kind: "sol.struct",
            children: &[("members", "member")],
            names: &["name"],
            structure: &["visibility"],
            ..NodeSpec::EMPTY
        },
        "EnumDefinition" => NodeSpec {
            kind: "sol.enum",
            children: &[("members", "member")],
            names: &["name"],
            ..NodeSpec::EMPTY
        },
        "EnumValue" => NodeSpec {
            kind: "sol.enum-value",
            names: &["name"],
            ..NodeSpec::EMPTY
        },
        "UserDefinedValueTypeDefinition" => NodeSpec {
            kind: "sol.udvt",
            children: &[("underlyingType", "type")],
            names: &["name"],
            ..NodeSpec::EMPTY
        },
        "EventDefinition" => NodeSpec {
            kind: "sol.event",
            children: &[("parameters", "params")],
            names: &["name"],
            structure: &["anonymous"],
            ..NodeSpec::EMPTY
        },
        "ErrorDefinition" => NodeSpec {
            kind: "sol.error",
            children: &[("parameters", "params")],
            names: &["name"],
            ..NodeSpec::EMPTY
        },
        "OverrideSpecifier" => NodeSpec {
            kind: "sol.override",
            children: &[("overrides", "target")],
            ..NodeSpec::EMPTY
        },
        // ------------------------------------------------------- statements
        "Block" => NodeSpec {
            kind: "sol.block",
            children: &[("statements", "stmt")],
            ..NodeSpec::EMPTY
        },
        "UncheckedBlock" => NodeSpec {
            kind: "sol.unchecked",
            children: &[("statements", "stmt")],
            ..NodeSpec::EMPTY
        },
        "ExpressionStatement" => NodeSpec {
            kind: "sol.expr-stmt",
            children: &[("expression", "expr")],
            ..NodeSpec::EMPTY
        },
        "VariableDeclarationStatement" => NodeSpec {
            kind: "sol.let",
            children: &[("declarations", "decl"), ("initialValue", "init")],
            ..NodeSpec::EMPTY
        },
        "IfStatement" => NodeSpec {
            kind: "sol.if",
            children: &[
                ("condition", "cond"),
                ("trueBody", "then"),
                ("falseBody", "else"),
            ],
            ..NodeSpec::EMPTY
        },
        "ForStatement" => NodeSpec {
            kind: "sol.for",
            children: &[
                ("initializationExpression", "init"),
                ("condition", "cond"),
                ("loopExpression", "post"),
                ("body", "body"),
            ],
            ..NodeSpec::EMPTY
        },
        "WhileStatement" => NodeSpec {
            kind: "sol.while",
            children: &[("condition", "cond"), ("body", "body")],
            ..NodeSpec::EMPTY
        },
        "DoWhileStatement" => NodeSpec {
            kind: "sol.do-while",
            children: &[("condition", "cond"), ("body", "body")],
            ..NodeSpec::EMPTY
        },
        "Return" => NodeSpec {
            kind: "sol.return",
            children: &[("expression", "expr")],
            ..NodeSpec::EMPTY
        },
        "Break" => NodeSpec {
            kind: "sol.break",
            ..NodeSpec::EMPTY
        },
        "Continue" => NodeSpec {
            kind: "sol.continue",
            ..NodeSpec::EMPTY
        },
        "EmitStatement" => NodeSpec {
            kind: "sol.emit",
            children: &[("eventCall", "call")],
            ..NodeSpec::EMPTY
        },
        "RevertStatement" => NodeSpec {
            kind: "sol.revert",
            children: &[("errorCall", "call")],
            ..NodeSpec::EMPTY
        },
        "TryStatement" => NodeSpec {
            kind: "sol.try",
            children: &[("externalCall", "call"), ("clauses", "clause")],
            ..NodeSpec::EMPTY
        },
        "TryCatchClause" => NodeSpec {
            kind: "sol.catch",
            children: &[("parameters", "params"), ("block", "body")],
            names: &["errorName"],
            ..NodeSpec::EMPTY
        },
        "PlaceholderStatement" => NodeSpec {
            kind: "sol.placeholder",
            ..NodeSpec::EMPTY
        },
        "InlineAssembly" => NodeSpec {
            kind: "sol.asm",
            children: &[("AST", "body")],
            ..NodeSpec::EMPTY
        },
        // ------------------------------------------------------ expressions
        "Assignment" => NodeSpec {
            kind: "sol.assign",
            children: &[("leftHandSide", "lhs"), ("rightHandSide", "rhs")],
            structure: &["operator"],
            types: TS,
            ..NodeSpec::EMPTY
        },
        "BinaryOperation" => NodeSpec {
            kind: "sol.binop",
            children: &[("leftExpression", "lhs"), ("rightExpression", "rhs")],
            structure: &["operator"],
            types: TS,
            ..NodeSpec::EMPTY
        },
        "UnaryOperation" => NodeSpec {
            kind: "sol.unop",
            children: &[("subExpression", "expr")],
            structure: &["operator", "prefix"],
            types: TS,
            ..NodeSpec::EMPTY
        },
        "Conditional" => NodeSpec {
            kind: "sol.ternary",
            children: &[
                ("condition", "cond"),
                ("trueExpression", "then"),
                ("falseExpression", "else"),
            ],
            types: TS,
            ..NodeSpec::EMPTY
        },
        "TupleExpression" => NodeSpec {
            kind: "sol.tuple",
            children: &[("components", "item")],
            structure: &["isInlineArray"],
            types: TS,
            ..NodeSpec::EMPTY
        },
        "FunctionCall" => NodeSpec {
            kind: "sol.call",
            children: &[("expression", "callee"), ("arguments", "arg")],
            // `names` (named arguments) map argument expressions to
            // parameters, ORDER-SENSITIVELY: f({x: a, y: b}) and
            // f({y: a, x: b}) differ. The walker emits the array as indexed
            // Names fields (names0, names1, …). Residual, documented: the
            // names-blind facet still blurs this binding (external review
            // P2; full canonicalization would reorder args by name).
            names: &["names"],
            structure: &["kind", "tryCall"],
            types: TS,
            ..NodeSpec::EMPTY
        },
        "FunctionCallOptions" => NodeSpec {
            kind: "sol.call-options",
            children: &[("expression", "callee"), ("options", "opt")],
            names: &["names"],
            types: TS,
            ..NodeSpec::EMPTY
        },
        "MemberAccess" => NodeSpec {
            kind: "sol.member",
            children: &[("expression", "expr")],
            names: &["memberName"],
            types: TS,
            ..NodeSpec::EMPTY
        },
        "IndexAccess" => NodeSpec {
            kind: "sol.index",
            children: &[("baseExpression", "base"), ("indexExpression", "index")],
            types: TS,
            ..NodeSpec::EMPTY
        },
        "IndexRangeAccess" => NodeSpec {
            kind: "sol.slice",
            children: &[
                ("baseExpression", "base"),
                ("startExpression", "start"),
                ("endExpression", "end"),
            ],
            types: TS,
            ..NodeSpec::EMPTY
        },
        "Identifier" => NodeSpec {
            kind: "sol.ident",
            names: &["name"],
            types: TS,
            ..NodeSpec::EMPTY
        },
        "IdentifierPath" => NodeSpec {
            kind: "sol.ident-path",
            names: &["name"],
            ..NodeSpec::EMPTY
        },
        "Literal" => NodeSpec {
            kind: "sol.lit",
            structure: &["kind", "subdenomination"],
            constants: &[ConstExtract::SolLiteral],
            types: TS,
            ..NodeSpec::EMPTY
        },
        "ElementaryTypeNameExpression" => NodeSpec {
            kind: "sol.type-expr",
            children: &[("typeName", "type")],
            types: TS,
            ..NodeSpec::EMPTY
        },
        "NewExpression" => NodeSpec {
            kind: "sol.new",
            children: &[("typeName", "type")],
            types: TS,
            ..NodeSpec::EMPTY
        },
        // ------------------------------------------------------- type names
        "ElementaryTypeName" => NodeSpec {
            kind: "sol.type.elementary",
            structure: &["stateMutability"],
            types: &[TypeExtract::Field("name")],
            ..NodeSpec::EMPTY
        },
        "ArrayTypeName" => NodeSpec {
            kind: "sol.type.array",
            children: &[("baseType", "base"), ("length", "len")],
            ..NodeSpec::EMPTY
        },
        "Mapping" => NodeSpec {
            kind: "sol.type.mapping",
            children: &[("keyType", "key"), ("valueType", "value")],
            names: &["keyName", "valueName"],
            ..NodeSpec::EMPTY
        },
        "UserDefinedTypeName" => NodeSpec {
            kind: "sol.type.user",
            children: &[("pathNode", "path")],
            types: TS,
            ..NodeSpec::EMPTY
        },
        "FunctionTypeName" => NodeSpec {
            kind: "sol.type.function",
            children: &[
                ("parameterTypes", "params"),
                ("returnParameterTypes", "returns"),
            ],
            structure: &["stateMutability", "visibility"],
            ..NodeSpec::EMPTY
        },
        // ------------------------------------- Yul inside InlineAssembly
        "YulBlock" => NodeSpec {
            kind: "yul.block",
            children: &[("statements", "stmt")],
            ..NodeSpec::EMPTY
        },
        "YulFunctionDefinition" => NodeSpec {
            kind: "yul.function",
            children: &[
                ("parameters", "param"),
                ("returnVariables", "ret"),
                ("body", "body"),
            ],
            names: &["name"],
            ..NodeSpec::EMPTY
        },
        "YulVariableDeclaration" => NodeSpec {
            kind: "yul.let",
            children: &[("variables", "var"), ("value", "value")],
            ..NodeSpec::EMPTY
        },
        "YulAssignment" => NodeSpec {
            kind: "yul.assign",
            children: &[("variableNames", "target"), ("value", "value")],
            ..NodeSpec::EMPTY
        },
        "YulExpressionStatement" => NodeSpec {
            kind: "yul.expr-stmt",
            children: &[("expression", "expr")],
            ..NodeSpec::EMPTY
        },
        "YulIf" => NodeSpec {
            kind: "yul.if",
            children: &[("condition", "cond"), ("body", "body")],
            ..NodeSpec::EMPTY
        },
        "YulSwitch" => NodeSpec {
            kind: "yul.switch",
            children: &[("expression", "scrutinee"), ("cases", "case")],
            ..NodeSpec::EMPTY
        },
        "YulCase" => NodeSpec {
            kind: "yul.case",
            // `value` is a YulLiteral object for `case X` and the bare
            // string "default" for the default case — the walker only
            // recurses into objects, so default cases simply lack the value
            // child (arity distinguishes them). Without this child,
            // `case 0`/`case 1`/`default` with equal bodies collided
            // (external review P2).
            children: &[("value", "value"), ("body", "body")],
            ..NodeSpec::EMPTY
        },
        "YulForLoop" => NodeSpec {
            kind: "yul.for",
            children: &[
                ("pre", "pre"),
                ("condition", "cond"),
                ("post", "post"),
                ("body", "body"),
            ],
            ..NodeSpec::EMPTY
        },
        "YulBreak" => NodeSpec {
            kind: "yul.break",
            ..NodeSpec::EMPTY
        },
        "YulContinue" => NodeSpec {
            kind: "yul.continue",
            ..NodeSpec::EMPTY
        },
        "YulLeave" => NodeSpec {
            kind: "yul.leave",
            ..NodeSpec::EMPTY
        },
        "YulFunctionCall" => NodeSpec {
            kind: "yul.call",
            children: &[("functionName", "callee"), ("arguments", "arg")],
            ..NodeSpec::EMPTY
        },
        "YulIdentifier" => NodeSpec {
            kind: "yul.ident",
            names: &["name"],
            ..NodeSpec::EMPTY
        },
        "YulTypedName" => NodeSpec {
            kind: "yul.typed-name",
            names: &["name"],
            ..NodeSpec::EMPTY
        },
        "YulLiteral" => NodeSpec {
            kind: "yul.lit",
            structure: &["kind"],
            constants: &[ConstExtract::YulLiteral],
            ..NodeSpec::EMPTY
        },
        "StructuredDocumentation" => NodeSpec {
            // doc text is not code; the node exists so strict mode passes,
            // but nothing is read from it.
            kind: "sol.doc",
            ..NodeSpec::EMPTY
        },
        _ => return None,
    };
    Some(spec)
}
