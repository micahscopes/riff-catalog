//! Canonical riff graphs for stable Fe runtime-MIR observations.
//!
//! Fe owns the runtime IR and emits an opt-in, content-addressed textual view.
//! This crate parses that boundary out of process. Neither lowering decisions
//! nor compiler cache keys depend on riffcat.

use std::collections::{BTreeMap, BTreeSet, HashMap, VecDeque};

use riff_catalog_core::{
    CatalogError, CyclePolicy, Digest, DigestRequest, Dimension, EdgeRole, EntityKey, Graph,
    GraphKey, HashPolicy, NodeKey, ViewMode, digest_graph,
};
use sha2::{Digest as _, Sha256};
use thiserror::Error;

/// Complete typed runtime-MIR package topology.
pub const FE_RMIR_LEVEL: &str = "fe-rmir/1";

/// Aggregate construction, projection, materialization, and its data slice.
pub const FE_RMIR_AGGREGATE_LEVEL: &str = "fe-rmir-aggregate/1";

/// Direct aggregate and memory-materialization operations with their immediate
/// typed values. Unlike the aggregate data slice, this view does not retain
/// transitive producers, so it exposes where representation pressure enters.
pub const FE_RMIR_MATERIALIZATION_LEVEL: &str = "fe-rmir-materialization/1";

/// Call sites, immediate typed values, and callee relationships. This view
/// makes helper fan-out and potential inlining amplification independently
/// observable without retaining complete function bodies.
pub const FE_RMIR_CALL_LEVEL: &str = "fe-rmir-call/1";

/// One complete function body plus structurally fingerprinted direct callees.
/// This view makes post-erasure helper equivalence queryable without copying
/// every reachable callee body into every unit.
pub const FE_RMIR_FUNCTION_LEVEL: &str = "fe-rmir-function/1";

#[derive(Debug, Error)]
pub enum LowerError {
    #[error(transparent)]
    Catalog(#[from] CatalogError),
    #[error("invalid Fe runtime-IR observation at line {line}: {message}")]
    Parse { line: usize, message: String },
    #[error("unsupported Fe runtime-IR observation format `{0}`")]
    UnsupportedFormat(String),
    #[error("runtime-IR observation digest mismatch: header {expected}, computed {actual}")]
    DigestMismatch { expected: String, actual: String },
    #[error("function {function} references undeclared local %{local}")]
    MissingLocal { function: String, local: u32 },
    #[error("function {function} references missing block bb{block}")]
    MissingBlock { function: String, block: u32 },
}

#[derive(Clone, Debug)]
pub struct LoweredRmir {
    pub graph_key: GraphKey,
    pub graph: Graph,
    pub function_count: usize,
    pub block_count: usize,
    pub statement_count: usize,
    pub call_count: usize,
}

#[derive(Clone, Debug)]
pub struct LoweredRmirFunction {
    pub symbol: String,
    pub instance: String,
    pub lowered: LoweredRmir,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ShapeClass {
    pub digest: Digest,
    pub occurrences: usize,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StructureCensus {
    pub nodes: usize,
    pub distinct_shapes: usize,
    pub repeated_occurrences: usize,
    pub largest_class: usize,
    pub classes: Vec<ShapeClass>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FunctionShapeCensus {
    pub functions: usize,
    pub distinct_shapes: usize,
    pub repeated_occurrences: usize,
    pub largest_class: usize,
    pub classes: Vec<ShapeClass>,
}

/// All independently useful projections derived from one checked snapshot.
/// Keeping them together guarantees that every view describes the same input
/// bytes and avoids reparsing multi-megabyte compiler observations.
#[derive(Clone, Debug)]
pub struct RmirViews {
    pub package: LoweredRmir,
    pub aggregate: LoweredRmir,
    pub materialization: LoweredRmir,
    pub calls: LoweredRmir,
}

/// Parse once, then derive the complete package graph and aggregate projection.
pub fn parse_and_lower_views(
    owner: &str,
    source: &str,
) -> Result<(LoweredRmir, LoweredRmir), LowerError> {
    let views = parse_and_lower_analysis_views(owner, source)?;
    Ok((views.package, views.aggregate))
}

/// Parse once, then derive the complete package plus focused pressure and call
/// frontiers. The older two-view API remains available for existing callers.
pub fn parse_and_lower_analysis_views(owner: &str, source: &str) -> Result<RmirViews, LowerError> {
    let package = parse_snapshot(source)?;
    let lowered = lower_package(owner, &package)?;
    let aggregate = project_aggregate_package(owner, &lowered)?;
    let materialization = project_materialization_package(owner, &lowered)?;
    let calls = project_call_package(owner, &lowered)?;
    Ok(RmirViews {
        package: lowered,
        aggregate,
        materialization,
        calls,
    })
}

pub fn structure_census(lowered: &LoweredRmir) -> Result<StructureCensus, LowerError> {
    structure_census_at_level(lowered, FE_RMIR_LEVEL)
}

pub fn aggregate_structure_census(lowered: &LoweredRmir) -> Result<StructureCensus, LowerError> {
    structure_census_at_level(lowered, FE_RMIR_AGGREGATE_LEVEL)
}

pub fn materialization_structure_census(
    lowered: &LoweredRmir,
) -> Result<StructureCensus, LowerError> {
    structure_census_at_level(lowered, FE_RMIR_MATERIALIZATION_LEVEL)
}

pub fn call_structure_census(lowered: &LoweredRmir) -> Result<StructureCensus, LowerError> {
    structure_census_at_level(lowered, FE_RMIR_CALL_LEVEL)
}

pub fn function_structure_census(
    functions: &[LoweredRmirFunction],
) -> Result<FunctionShapeCensus, LowerError> {
    let mut counts = BTreeMap::<Digest, usize>::new();
    for function in functions {
        *counts
            .entry(function_graph_structure_digest(&function.lowered)?)
            .or_default() += 1;
    }
    let mut classes = counts
        .into_iter()
        .map(|(digest, occurrences)| ShapeClass {
            digest,
            occurrences,
        })
        .collect::<Vec<_>>();
    classes.sort_by(|left, right| {
        right
            .occurrences
            .cmp(&left.occurrences)
            .then_with(|| left.digest.cmp(&right.digest))
    });
    let functions = functions.len();
    let distinct_shapes = classes.len();
    Ok(FunctionShapeCensus {
        functions,
        distinct_shapes,
        repeated_occurrences: functions.saturating_sub(distinct_shapes),
        largest_class: classes.first().map_or(0, |class| class.occurrences),
        classes,
    })
}

fn function_graph_structure_digest(lowered: &LoweredRmir) -> Result<Digest, LowerError> {
    Ok(*function_graph_digests(lowered, &[Dimension::Structure])?
        .get(&Dimension::Structure)
        .expect("the requested structure digest must be present"))
}

fn function_graph_digests(
    lowered: &LoweredRmir,
    dimensions: &[Dimension],
) -> Result<BTreeMap<Dimension, Digest>, LowerError> {
    let policy = HashPolicy::new(
        FE_RMIR_FUNCTION_LEVEL,
        ViewMode::AnonymousShape,
        CyclePolicy::CondenseScc,
    )?;
    let result = digest_graph(
        &DigestRequest::new(
            lowered.graph_key.clone(),
            policy,
            dimensions.iter().copied(),
        )?,
        &lowered.graph,
    )?;
    Ok(result.hashes.graph.values)
}

fn structure_census_at_level(
    lowered: &LoweredRmir,
    level: &str,
) -> Result<StructureCensus, LowerError> {
    let policy = HashPolicy::new(level, ViewMode::AnonymousShape, CyclePolicy::CondenseScc)?;
    let result = digest_graph(
        &DigestRequest::new(lowered.graph_key.clone(), policy, [Dimension::Structure])?,
        &lowered.graph,
    )?;
    let mut counts = BTreeMap::<Digest, usize>::new();
    for hashes in result.hashes.nodes.values() {
        let digest = *hashes
            .tree
            .get(Dimension::Structure)
            .expect("the requested structure digest must be present");
        *counts.entry(digest).or_default() += 1;
    }
    let mut classes = counts
        .into_iter()
        .map(|(digest, occurrences)| ShapeClass {
            digest,
            occurrences,
        })
        .collect::<Vec<_>>();
    classes.sort_by(|left, right| {
        right
            .occurrences
            .cmp(&left.occurrences)
            .then_with(|| left.digest.cmp(&right.digest))
    });
    let nodes = result.hashes.nodes.len();
    let distinct_shapes = classes.len();
    Ok(StructureCensus {
        nodes,
        distinct_shapes,
        repeated_occurrences: nodes.saturating_sub(distinct_shapes),
        largest_class: classes.first().map_or(0, |class| class.occurrences),
        classes,
    })
}

#[derive(Clone, Debug)]
struct ParsedPackage {
    name: String,
    primary_object: Option<String>,
    root_objects: Vec<String>,
    const_regions: u32,
    functions: Vec<ParsedFunction>,
}

#[derive(Clone, Debug)]
struct ParsedFunction {
    symbol: String,
    linkage: String,
    inline_hint: String,
    instance: String,
    result: String,
    params: Vec<ParsedParam>,
    providers: Vec<ParsedProvider>,
    locals: Vec<ParsedLocal>,
    blocks: Vec<ParsedBlock>,
}

#[derive(Clone, Debug)]
struct ParsedParam {
    local: u32,
    class: String,
}

#[derive(Clone, Debug)]
struct ParsedProvider {
    index: u32,
    value: u32,
    provider: String,
    place: String,
}

#[derive(Clone, Debug)]
struct ParsedLocal {
    id: u32,
    ty: String,
    carrier: String,
    root: String,
}

#[derive(Clone, Debug)]
struct ParsedBlock {
    id: u32,
    statements: Vec<ParsedStatement>,
    terminator: ParsedTerminator,
}

#[derive(Clone, Debug)]
struct ParsedStatement {
    ordinal: u32,
    operation: String,
    destination: Option<u32>,
    uses: Vec<u32>,
    call_target: Option<String>,
    constant: Option<String>,
    type_hint: Option<String>,
}

#[derive(Clone, Debug)]
struct ParsedTerminator {
    operation: String,
    uses: Vec<u32>,
    targets: Vec<u32>,
    call_target: Option<String>,
}

#[derive(Clone, Debug)]
struct FunctionSummary {
    symbol: String,
    linkage: String,
    inline_hint: String,
    instance: String,
}

fn parse_snapshot(source: &str) -> Result<ParsedPackage, LowerError> {
    let mut sections = source.splitn(4, '\n');
    let format = sections.next().unwrap_or_default();
    if format != "# fe-rmir-observation/1" {
        return Err(LowerError::UnsupportedFormat(format.to_string()));
    }
    let phase = sections.next().unwrap_or_default();
    if phase != "# phase: runtime-package" {
        return Err(LowerError::UnsupportedFormat(phase.to_string()));
    }
    let digest_line = sections.next().unwrap_or_default();
    let expected = digest_line
        .strip_prefix("# sha256: ")
        .ok_or_else(|| LowerError::UnsupportedFormat(digest_line.to_string()))?;
    let body_with_newline = sections.next().unwrap_or_default();
    let body = body_with_newline
        .strip_suffix('\n')
        .unwrap_or(body_with_newline);
    let actual = format!("{:x}", Sha256::digest(body.as_bytes()));
    if expected != actual {
        return Err(LowerError::DigestMismatch {
            expected: expected.to_string(),
            actual,
        });
    }
    parse_package_body(body)
}

fn parse_package_body(body: &str) -> Result<ParsedPackage, LowerError> {
    let lines = body.lines().collect::<Vec<_>>();
    let first = lines.first().copied().unwrap_or_default();
    let name = first
        .strip_prefix("package ")
        .and_then(|line| line.strip_suffix(" {"))
        .ok_or_else(|| parse_error(1, "expected `package NAME {`"))?
        .to_string();
    let mut package = ParsedPackage {
        name,
        primary_object: None,
        root_objects: Vec::new(),
        const_regions: 0,
        functions: Vec::new(),
    };
    let mut index = 1;
    while index < lines.len() {
        let line = lines[index];
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed == "functions:" {
            index += 1;
            continue;
        }
        if trimmed == "}" {
            break;
        }
        if let Some(value) = trimmed.strip_prefix("primary_object: ") {
            package.primary_object = Some(value.to_string());
            index += 1;
            continue;
        }
        if let Some(value) = trimmed.strip_prefix("root_objects: [") {
            let value = value
                .strip_suffix(']')
                .ok_or_else(|| parse_error(index + 1, "unterminated root object list"))?;
            package.root_objects = split_list(value);
            index += 1;
            continue;
        }
        if let Some(value) = trimmed.strip_prefix("const_regions: ") {
            package.const_regions = parse_u32(index + 1, value, "const region count")?;
            index += 1;
            continue;
        }
        if line.starts_with("    ") && !line.starts_with("      ") {
            let summary = parse_function_summary(index + 1, trimmed)?;
            index += 1;
            package
                .functions
                .push(parse_function(&lines, &mut index, summary)?);
            continue;
        }
        return Err(parse_error(
            index + 1,
            format!("unexpected package line `{trimmed}`"),
        ));
    }
    Ok(package)
}

fn parse_function_summary(line: usize, text: &str) -> Result<FunctionSummary, LowerError> {
    let (symbol, metadata) = text
        .split_once(" (")
        .ok_or_else(|| parse_error(line, "malformed function summary"))?;
    let metadata = metadata
        .strip_suffix(')')
        .ok_or_else(|| parse_error(line, "unterminated function summary"))?;
    let (linkage, rest) = metadata
        .split_once(", inline=")
        .ok_or_else(|| parse_error(line, "function summary has no inline hint"))?;
    let (inline_hint, instance) = rest
        .split_once(", instance=")
        .ok_or_else(|| parse_error(line, "function summary has no stable instance"))?;
    Ok(FunctionSummary {
        symbol: symbol.to_string(),
        linkage: linkage.to_string(),
        inline_hint: inline_hint.to_string(),
        instance: instance.to_string(),
    })
}

fn parse_function(
    lines: &[&str],
    index: &mut usize,
    summary: FunctionSummary,
) -> Result<ParsedFunction, LowerError> {
    let header_line = lines
        .get(*index)
        .copied()
        .ok_or_else(|| parse_error(*index + 1, "missing function body"))?;
    let instance = header_line
        .trim()
        .strip_prefix("fn ")
        .and_then(|line| line.strip_suffix('('))
        .ok_or_else(|| parse_error(*index + 1, "expected function body header"))?;
    if instance != summary.instance {
        return Err(parse_error(
            *index + 1,
            "function summary and body instance keys differ",
        ));
    }
    *index += 1;
    let mut params = Vec::new();
    let result;
    loop {
        let line = lines
            .get(*index)
            .copied()
            .ok_or_else(|| parse_error(*index + 1, "unterminated function signature"))?;
        let trimmed = line.trim();
        if let Some(value) = trimmed.strip_prefix(") -> ") {
            result = value
                .strip_suffix(" {")
                .ok_or_else(|| parse_error(*index + 1, "malformed function result"))?
                .to_string();
            *index += 1;
            break;
        }
        let declaration = trimmed.strip_suffix(',').unwrap_or(trimmed);
        let (local, class) = declaration
            .split_once(": ")
            .ok_or_else(|| parse_error(*index + 1, "malformed function parameter"))?;
        params.push(ParsedParam {
            local: parse_local(*index + 1, local)?,
            class: class.to_string(),
        });
        *index += 1;
    }

    let mut providers = Vec::new();
    let mut locals = Vec::new();
    let mut blocks = Vec::new();
    let mut current_block: Option<ParsedBlock> = None;
    while *index < lines.len() {
        let line = lines[*index];
        let trimmed = line.trim();
        if line == "      }" {
            if let Some(block) = current_block.take() {
                blocks.push(block);
            }
            *index += 1;
            break;
        }
        if trimmed.is_empty() || matches!(trimmed, "providers:" | "locals:") {
            *index += 1;
            continue;
        }
        if trimmed.starts_with('@') {
            providers.push(parse_provider(*index + 1, trimmed)?);
            *index += 1;
            continue;
        }
        if trimmed.starts_with('%') && trimmed.contains(": ty=") {
            locals.push(parse_local_decl(*index + 1, trimmed)?);
            *index += 1;
            continue;
        }
        if let Some(block) = parse_block_header(*index + 1, trimmed)? {
            if let Some(previous) = current_block.replace(ParsedBlock {
                id: block,
                statements: Vec::new(),
                terminator: ParsedTerminator {
                    operation: "missing".to_string(),
                    uses: Vec::new(),
                    targets: Vec::new(),
                    call_target: None,
                },
            }) {
                if previous.terminator.operation == "missing" {
                    return Err(parse_error(*index + 1, "block has no terminator"));
                }
                blocks.push(previous);
            }
            *index += 1;
            continue;
        }
        if trimmed.starts_with('[') {
            let block = current_block
                .as_mut()
                .ok_or_else(|| parse_error(*index + 1, "statement outside a block"))?;
            block.statements.push(parse_statement(*index + 1, trimmed)?);
            *index += 1;
            continue;
        }
        if let Some(text) = trimmed.strip_prefix("-> ") {
            let block = current_block
                .as_mut()
                .ok_or_else(|| parse_error(*index + 1, "terminator outside a block"))?;
            block.terminator = parse_terminator(text);
            *index += 1;
            continue;
        }
        return Err(parse_error(
            *index + 1,
            format!("unexpected function line `{trimmed}`"),
        ));
    }
    if blocks
        .iter()
        .any(|block| block.terminator.operation == "missing")
    {
        return Err(parse_error(*index, "block has no terminator"));
    }
    Ok(ParsedFunction {
        symbol: summary.symbol,
        linkage: summary.linkage,
        inline_hint: summary.inline_hint,
        instance: summary.instance,
        result,
        params,
        providers,
        locals,
        blocks,
    })
}

fn parse_provider(line: usize, text: &str) -> Result<ParsedProvider, LowerError> {
    let (index, rest) = text
        .split_once(" => value=%")
        .ok_or_else(|| parse_error(line, "malformed provider binding"))?;
    let (value, rest) = rest
        .split_once(", provider=")
        .ok_or_else(|| parse_error(line, "provider binding has no provider type"))?;
    let (provider, place) = rest
        .split_once(", place=")
        .ok_or_else(|| parse_error(line, "provider binding has no place type"))?;
    Ok(ParsedProvider {
        index: parse_u32(line, index.trim_start_matches('@'), "provider index")?,
        value: parse_u32(line, value, "provider local")?,
        provider: provider.to_string(),
        place: place.to_string(),
    })
}

fn parse_local_decl(line: usize, text: &str) -> Result<ParsedLocal, LowerError> {
    let (id, rest) = text
        .split_once(": ty=")
        .ok_or_else(|| parse_error(line, "malformed local declaration"))?;
    let (ty, rest) = rest
        .split_once(", carrier=")
        .ok_or_else(|| parse_error(line, "local declaration has no carrier"))?;
    let (carrier, root) = rest
        .split_once(", root=")
        .ok_or_else(|| parse_error(line, "local declaration has no root"))?;
    Ok(ParsedLocal {
        id: parse_local(line, id)?,
        ty: ty.to_string(),
        carrier: carrier.to_string(),
        root: root.to_string(),
    })
}

fn parse_block_header(line: usize, text: &str) -> Result<Option<u32>, LowerError> {
    let Some(value) = text.strip_prefix("bb") else {
        return Ok(None);
    };
    let Some(value) = value.strip_suffix(':') else {
        return Ok(None);
    };
    Ok(Some(parse_u32(line, value, "block index")?))
}

fn parse_statement(line: usize, text: &str) -> Result<ParsedStatement, LowerError> {
    let (ordinal, rest) = text
        .strip_prefix('[')
        .and_then(|value| value.split_once("] "))
        .ok_or_else(|| parse_error(line, "malformed statement ordinal"))?;
    let text = rest
        .strip_suffix(';')
        .ok_or_else(|| parse_error(line, "statement has no trailing semicolon"))?;
    let (destination, expression) = match text.split_once(" = ") {
        Some((destination, expression)) if destination.starts_with('%') => {
            (Some(parse_local(line, destination)?), expression)
        }
        _ => (None, text),
    };
    let operation = expression_operation(expression);
    Ok(ParsedStatement {
        ordinal: parse_u32(line, ordinal, "statement ordinal")?,
        operation,
        destination,
        uses: local_references(expression),
        call_target: call_target(expression, "call "),
        constant: expression_constant(expression),
        type_hint: expression_type_hint(expression),
    })
}

fn parse_terminator(text: &str) -> ParsedTerminator {
    let operation = text
        .split_whitespace()
        .next()
        .unwrap_or("unknown")
        .to_string();
    ParsedTerminator {
        operation,
        uses: local_references(text),
        targets: block_references(text),
        call_target: call_target(text, "terminal_call "),
    }
}

fn expression_operation(text: &str) -> String {
    if expression_constant(text).is_some() {
        "const_scalar".to_string()
    } else {
        text.split_whitespace()
            .next()
            .unwrap_or("unknown")
            .to_string()
    }
}

fn expression_constant(text: &str) -> Option<String> {
    if matches!(text, "true" | "false") {
        return Some(text.to_string());
    }
    let prefix = text.split_once('(')?.0;
    let scalar = prefix == "bytes"
        || prefix.starts_with("address")
        || prefix.starts_with('f') && prefix[1..].chars().all(|ch| ch.is_ascii_digit())
        || prefix.ends_with("int")
        || prefix.contains("int") && prefix.chars().all(|ch| ch.is_ascii_alphanumeric());
    scalar.then(|| text.to_string())
}

fn expression_type_hint(text: &str) -> Option<String> {
    for prefix in ["aggregate_make ", "enum_make "] {
        if let Some(rest) = text.strip_prefix(prefix) {
            return rest.rsplit_once('(').map(|(layout, _)| layout.to_string());
        }
    }
    for prefix in ["alloc ", "const_ref ", "placeholder "] {
        if let Some(rest) = text.strip_prefix(prefix) {
            return Some(rest.to_string());
        }
    }
    for marker in [" as ", " -> "] {
        if let Some((_, ty)) = text.rsplit_once(marker) {
            return Some(ty.to_string());
        }
    }
    None
}

fn call_target(text: &str, prefix: &str) -> Option<String> {
    text.strip_prefix(prefix)?
        .rsplit_once('(')
        .map(|(target, _)| target.to_string())
}

fn local_references(text: &str) -> Vec<u32> {
    numeric_references(text, '%')
}

fn block_references(text: &str) -> Vec<u32> {
    let bytes = text.as_bytes();
    let mut values = Vec::new();
    let mut index = 0;
    while index + 2 <= bytes.len() {
        if bytes[index] == b'b' && bytes.get(index + 1) == Some(&b'b') {
            let start = index + 2;
            let mut end = start;
            while bytes.get(end).is_some_and(u8::is_ascii_digit) {
                end += 1;
            }
            if end > start {
                if let Ok(value) = text[start..end].parse() {
                    values.push(value);
                }
                index = end;
                continue;
            }
        }
        index += 1;
    }
    values
}

fn numeric_references(text: &str, marker: char) -> Vec<u32> {
    let bytes = text.as_bytes();
    let marker = marker as u8;
    let mut values = Vec::new();
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == marker {
            let start = index + 1;
            let mut end = start;
            while bytes.get(end).is_some_and(u8::is_ascii_digit) {
                end += 1;
            }
            if end > start {
                if let Ok(value) = text[start..end].parse() {
                    values.push(value);
                }
                index = end;
                continue;
            }
        }
        index += 1;
    }
    values
}

fn split_list(text: &str) -> Vec<String> {
    if text.is_empty() {
        Vec::new()
    } else {
        text.split(", ").map(ToString::to_string).collect()
    }
}

fn parse_local(line: usize, text: &str) -> Result<u32, LowerError> {
    parse_u32(line, text.trim_start_matches('%'), "local index")
}

fn parse_u32(line: usize, text: &str, what: &str) -> Result<u32, LowerError> {
    text.parse()
        .map_err(|_| parse_error(line, format!("invalid {what} `{text}`")))
}

fn parse_error(line: usize, message: impl Into<String>) -> LowerError {
    LowerError::Parse {
        line,
        message: message.into(),
    }
}

fn lower_package(owner: &str, package: &ParsedPackage) -> Result<LoweredRmir, LowerError> {
    let package_entity = EntityKey::new("fe.rmir.package", owner, "package")?;
    let graph_key = GraphKey::new(package_entity.clone(), "package")?;
    let root = NodeKey::entity(package_entity);
    let mut graph = Graph::new(graph_key.clone());
    graph.add_node(root.clone(), "fe.rmir.package")?;
    graph.add_field(&root, Dimension::Names, "name", package.name.clone())?;
    graph.add_field(
        &root,
        Dimension::Structure,
        "const_regions",
        package.const_regions,
    )?;
    if let Some(primary) = &package.primary_object {
        graph.add_field(&root, Dimension::Names, "primary_object", primary.clone())?;
    }
    for (index, object) in package.root_objects.iter().enumerate() {
        graph.add_field(
            &root,
            Dimension::Names,
            format!("root_object:{index}"),
            object.clone(),
        )?;
    }

    let mut function_nodes = Vec::new();
    let mut functions_by_instance = HashMap::new();
    for (ordinal, function) in package.functions.iter().enumerate() {
        let entity = EntityKey::new("fe.rmir.function", owner, format!("function:{ordinal}"))?;
        let node = NodeKey::entity(entity);
        graph.add_node(node.clone(), "fe.rmir.function")?;
        graph.add_field(&node, Dimension::Names, "symbol", function.symbol.clone())?;
        graph.add_field(
            &node,
            Dimension::Names,
            "instance",
            function.instance.clone(),
        )?;
        graph.add_field(
            &node,
            Dimension::Structure,
            "linkage",
            function.linkage.clone(),
        )?;
        graph.add_field(
            &node,
            Dimension::Structure,
            "inline",
            function.inline_hint.clone(),
        )?;
        graph.add_field(&node, Dimension::Types, "result", function.result.clone())?;
        for (index, param) in function.params.iter().enumerate() {
            graph.add_field(
                &node,
                Dimension::Types,
                format!("parameter:{index}"),
                param.class.clone(),
            )?;
        }
        graph.add_child(&root, "function", ordinal as u32, &node)?;
        functions_by_instance.insert(function.instance.clone(), node.clone());
        function_nodes.push(node);
    }

    let mut external_functions = HashMap::<String, NodeKey>::new();
    let mut block_count = 0;
    let mut statement_count = 0;
    let mut call_count = 0;
    for (function_ordinal, function) in package.functions.iter().enumerate() {
        let function_node = &function_nodes[function_ordinal];
        let function_owner = function_node.owner().clone();
        let params = function
            .params
            .iter()
            .enumerate()
            .map(|(index, param)| (param.local, index))
            .collect::<HashMap<_, _>>();
        let mut local_nodes = HashMap::new();
        for (ordinal, local) in function.locals.iter().enumerate() {
            let node = NodeKey::derived(function_owner.clone(), format!("local:{}", local.id))?;
            graph.add_node(node.clone(), "fe.rmir.local")?;
            graph.add_field(&node, Dimension::Types, "semantic", local.ty.clone())?;
            graph.add_field(&node, Dimension::Types, "carrier", local.carrier.clone())?;
            graph.add_field(&node, Dimension::Types, "root", local.root.clone())?;
            if let Some(index) = params.get(&local.id) {
                graph.add_field(
                    &node,
                    Dimension::Structure,
                    "parameter_index",
                    *index as u64,
                )?;
            }
            graph.add_child(function_node, "local", ordinal as u32, &node)?;
            local_nodes.insert(local.id, node);
        }
        for param in &function.params {
            if !local_nodes.contains_key(&param.local) {
                return Err(LowerError::MissingLocal {
                    function: function.symbol.clone(),
                    local: param.local,
                });
            }
        }

        for provider in &function.providers {
            let node = NodeKey::derived(
                function_owner.clone(),
                format!("provider:{}", provider.index),
            )?;
            graph.add_node(node.clone(), "fe.rmir.provider")?;
            graph.add_field(
                &node,
                Dimension::Types,
                "provider",
                provider.provider.clone(),
            )?;
            graph.add_field(&node, Dimension::Types, "place", provider.place.clone())?;
            graph.add_child(function_node, "provider", provider.index, &node)?;
            let value = require_local(function, &local_nodes, provider.value)?;
            graph.add_edge(value, "value", &node, EdgeRole::Data)?;
        }

        let mut block_nodes = HashMap::new();
        for (ordinal, block) in function.blocks.iter().enumerate() {
            let node = NodeKey::derived(function_owner.clone(), format!("block:{}", block.id))?;
            graph.add_node(node.clone(), "fe.rmir.block")?;
            graph.add_child(function_node, "block", ordinal as u32, &node)?;
            block_nodes.insert(block.id, node);
            block_count += 1;
        }

        for block in &function.blocks {
            let block_node = block_nodes
                .get(&block.id)
                .expect("block nodes were predeclared");
            for statement in &block.statements {
                let node = NodeKey::derived(
                    function_owner.clone(),
                    format!("block:{}:statement:{}", block.id, statement.ordinal),
                )?;
                graph.add_node(node.clone(), "fe.rmir.statement")?;
                graph.add_field(
                    &node,
                    Dimension::Structure,
                    "operation",
                    statement.operation.clone(),
                )?;
                if let Some(constant) = &statement.constant {
                    graph.add_field(&node, Dimension::Constants, "value", constant.clone())?;
                }
                if let Some(type_hint) = &statement.type_hint {
                    graph.add_field(&node, Dimension::Types, "type", type_hint.clone())?;
                }
                graph.add_child(block_node, "statement", statement.ordinal, &node)?;
                for (index, local) in statement.uses.iter().enumerate() {
                    let local = require_local(function, &local_nodes, *local)?;
                    graph.add_edge(local, format!("operand:{index}"), &node, EdgeRole::Data)?;
                }
                if let Some(destination) = statement.destination {
                    let destination = require_local(function, &local_nodes, destination)?;
                    graph.add_edge(&node, "result", destination, EdgeRole::Data)?;
                }
                if let Some(target) = &statement.call_target {
                    let target_node = call_node(
                        &mut graph,
                        &root,
                        owner,
                        &functions_by_instance,
                        &mut external_functions,
                        target,
                    )?;
                    graph.add_edge(&node, "callee", &target_node, EdgeRole::Call)?;
                    call_count += 1;
                }
                statement_count += 1;
            }
            let terminator = &block.terminator;
            let node = NodeKey::derived(
                function_owner.clone(),
                format!("block:{}:terminator", block.id),
            )?;
            graph.add_node(node.clone(), "fe.rmir.terminator")?;
            graph.add_field(
                &node,
                Dimension::Structure,
                "operation",
                terminator.operation.clone(),
            )?;
            graph.add_child(
                block_node,
                "terminator",
                block.statements.len() as u32,
                &node,
            )?;
            for (index, local) in terminator.uses.iter().enumerate() {
                let local = require_local(function, &local_nodes, *local)?;
                graph.add_edge(local, format!("operand:{index}"), &node, EdgeRole::Data)?;
            }
            for (index, target) in terminator.targets.iter().enumerate() {
                let target = block_nodes
                    .get(target)
                    .ok_or_else(|| LowerError::MissingBlock {
                        function: function.symbol.clone(),
                        block: *target,
                    })?;
                graph.add_edge(
                    &node,
                    format!("successor:{index}"),
                    target,
                    EdgeRole::Control,
                )?;
            }
            if let Some(target) = &terminator.call_target {
                let target_node = call_node(
                    &mut graph,
                    &root,
                    owner,
                    &functions_by_instance,
                    &mut external_functions,
                    target,
                )?;
                graph.add_edge(&node, "callee", &target_node, EdgeRole::Call)?;
                call_count += 1;
            }
            statement_count += 1;
        }
    }
    graph.validate()?;
    Ok(LoweredRmir {
        graph_key,
        graph,
        function_count: package.functions.len(),
        block_count,
        statement_count,
        call_count,
    })
}

fn require_local<'a>(
    function: &ParsedFunction,
    locals: &'a HashMap<u32, NodeKey>,
    local: u32,
) -> Result<&'a NodeKey, LowerError> {
    locals.get(&local).ok_or_else(|| LowerError::MissingLocal {
        function: function.symbol.clone(),
        local,
    })
}

fn call_node(
    graph: &mut Graph,
    root: &NodeKey,
    owner: &str,
    functions: &HashMap<String, NodeKey>,
    external: &mut HashMap<String, NodeKey>,
    target: &str,
) -> Result<NodeKey, LowerError> {
    if let Some(node) = functions.get(target) {
        return Ok(node.clone());
    }
    if let Some(node) = external.get(target) {
        return Ok(node.clone());
    }
    let ordinal = external.len();
    let node = NodeKey::entity(EntityKey::new(
        "fe.rmir.external_function",
        owner,
        format!("external:{ordinal}"),
    )?);
    graph.add_node(node.clone(), "fe.rmir.external_function")?;
    graph.add_field(&node, Dimension::Names, "instance", target.to_string())?;
    graph.add_child(root, "external_function", ordinal as u32, &node)?;
    external.insert(target.to_string(), node.clone());
    Ok(node)
}

fn operation(node: &riff_catalog_core::Node) -> Option<&str> {
    node.fields.iter().find_map(|field| {
        if field.dimension == Dimension::Structure && field.name.as_str() == "operation" {
            match &field.value {
                riff_catalog_core::Value::Text(value) => Some(value.as_str()),
                _ => None,
            }
        } else {
            None
        }
    })
}

fn is_aggregate_operation(operation: &str) -> bool {
    matches!(
        operation,
        "aggregate_make"
            | "extract_value"
            | "alloc"
            | "materialize_to_object"
            | "materialize_place_to_object"
            | "addr_of"
            | "load"
            | "store"
            | "copy_into"
            | "enum_make"
            | "enum_extract"
            | "enum_write_variant"
            | "enum_set_tag"
            | "provider_from_raw"
            | "provider_to_raw"
            | "word_to_raw"
            | "retag_ref"
    )
}

/// Retain every aggregate operation, the complete data slice that produces its
/// inputs, and its package/function/block ancestry. The dependency topology is
/// preserved exactly; this projection never reassociates operations.
pub fn project_aggregate_package(
    owner: &str,
    lowered: &LoweredRmir,
) -> Result<LoweredRmir, LowerError> {
    let source = &lowered.graph;
    let mut selected = BTreeSet::new();
    let mut work = VecDeque::new();
    for (key, node) in &source.nodes {
        if operation(node).is_some_and(is_aggregate_operation) {
            selected.insert(key.clone());
            work.push_back(key.clone());
        }
    }
    let incoming_data = source
        .edges
        .iter()
        .filter(|edge| edge.role == EdgeRole::Data)
        .fold(HashMap::<NodeKey, Vec<_>>::new(), |mut by_target, edge| {
            by_target.entry(edge.target.clone()).or_default().push(edge);
            by_target
        });
    while let Some(key) = work.pop_front() {
        for edge in incoming_data.get(&key).into_iter().flatten() {
            if selected.insert(edge.source.clone()) {
                work.push_back(edge.source.clone());
            }
        }
    }
    include_ancestry(source, &mut selected);
    project_selected_package(owner, source, selected, "fe.rmir.aggregate", "aggregate")
}

/// Retain only direct aggregate and memory-materialization operations, their
/// immediate typed input and result values, and ancestry. This is deliberately
/// thinner than [`project_aggregate_package`]: producers do not enter merely
/// because their result eventually feeds a materialization.
pub fn project_materialization_package(
    owner: &str,
    lowered: &LoweredRmir,
) -> Result<LoweredRmir, LowerError> {
    let source = &lowered.graph;
    let frontier = source
        .nodes
        .iter()
        .filter_map(|(key, node)| {
            operation(node)
                .is_some_and(is_aggregate_operation)
                .then_some(key.clone())
        })
        .collect::<BTreeSet<_>>();
    let mut selected = frontier.clone();
    include_direct_data_neighbors(source, &frontier, &mut selected);
    include_ancestry(source, &mut selected);
    project_selected_package(
        owner,
        source,
        selected,
        "fe.rmir.materialization",
        "materialization",
    )
}

/// Retain call sites, their immediate typed values, callee nodes, and ancestry.
/// This is a compact view of helper fan-out and call-boundary representation.
pub fn project_call_package(owner: &str, lowered: &LoweredRmir) -> Result<LoweredRmir, LowerError> {
    let source = &lowered.graph;
    let call_edges = source
        .edges
        .iter()
        .filter(|edge| edge.role == EdgeRole::Call)
        .collect::<Vec<_>>();
    let frontier = call_edges
        .iter()
        .map(|edge| edge.source.clone())
        .collect::<BTreeSet<_>>();
    let mut selected = frontier.clone();
    for edge in call_edges {
        selected.insert(edge.target.clone());
    }
    include_direct_data_neighbors(source, &frontier, &mut selected);
    include_ancestry(source, &mut selected);
    project_selected_package(owner, source, selected, "fe.rmir.call", "call")
}

/// Split a checked package observation into independently hashable function
/// bodies. Direct callees are retained as lightweight stubs annotated with the
/// anonymous digest of their complete body in every facet dimension. This
/// preserves call semantics for equivalence analysis without recursively
/// copying every callee body into every caller unit.
pub fn project_function_packages(
    owner: &str,
    lowered: &LoweredRmir,
) -> Result<Vec<LoweredRmirFunction>, LowerError> {
    let source = &lowered.graph;
    let source_root = NodeKey::entity(source.graph_key.owner.clone());
    let package_policy = HashPolicy::new(
        FE_RMIR_LEVEL,
        ViewMode::AnonymousShape,
        CyclePolicy::CondenseScc,
    )?;
    let package_hashes = digest_graph(
        &DigestRequest::all_dimensions(lowered.graph_key.clone(), package_policy),
        source,
    )?
    .hashes;
    let mut function_children = source
        .children
        .iter()
        .filter(|child| child.parent == source_root && child.label.as_str() == "function")
        .collect::<Vec<_>>();
    function_children.sort_by_key(|child| child.ordinal);

    let mut functions = Vec::with_capacity(function_children.len());
    for child in function_children {
        let function_key = &child.child;
        let function_node = source
            .nodes
            .get(function_key)
            .expect("a package function child must name a node");
        let symbol = text_field(function_node, Dimension::Names, "symbol")
            .unwrap_or("<anonymous>")
            .to_string();
        let instance = text_field(function_node, Dimension::Names, "instance")
            .unwrap_or("<anonymous>")
            .to_string();
        let function_owner = function_key.owner();
        let mut selected = source
            .nodes
            .keys()
            .filter(|key| key.owner() == function_owner)
            .cloned()
            .collect::<BTreeSet<_>>();
        let callee_stubs = source
            .edges
            .iter()
            .filter(|edge| edge.role == EdgeRole::Call && selected.contains(&edge.source))
            .filter_map(|edge| (!selected.contains(&edge.target)).then_some(edge.target.clone()))
            .collect::<BTreeSet<_>>();
        selected.extend(callee_stubs.iter().cloned());

        let projection_owner = EntityKey::new(
            "fe.rmir.function_view",
            owner,
            format!("function:{}", child.ordinal),
        )?;
        let graph_key = GraphKey::new(projection_owner.clone(), "function")?;
        let root = NodeKey::entity(projection_owner);
        let mut graph = Graph::new(graph_key.clone());
        graph.add_node(root.clone(), "fe.rmir.function_view")?;
        graph.add_field(&root, Dimension::Names, "symbol", symbol.clone())?;
        graph.add_field(&root, Dimension::Names, "instance", instance.clone())?;

        for key in &selected {
            let node = source
                .nodes
                .get(key)
                .expect("a projected function node must remain in the source graph");
            graph.nodes.insert(key.clone(), node.clone());
        }
        for stub in &callee_stubs {
            let hashes = package_hashes
                .nodes
                .get(stub)
                .expect("a direct callee must have package digests");
            for dimension in Dimension::ALL {
                let digest = hashes
                    .tree
                    .get(dimension)
                    .expect("all requested callee dimensions must be present");
                graph.add_field(stub, dimension, "callee_body", digest.to_hex())?;
            }
        }
        graph.add_child(&root, "body", 0, function_key)?;
        graph.children.extend(
            source
                .children
                .iter()
                .filter(|edge| {
                    selected.contains(&edge.parent)
                        && selected.contains(&edge.child)
                        && !callee_stubs.contains(&edge.parent)
                })
                .cloned(),
        );
        graph.edges.extend(
            source
                .edges
                .iter()
                .filter(|edge| selected.contains(&edge.source) && selected.contains(&edge.target))
                .cloned(),
        );
        graph.validate()?;

        let block_count = graph
            .nodes
            .values()
            .filter(|node| node.kind.as_str() == "fe.rmir.block")
            .count();
        let statement_count = graph
            .nodes
            .values()
            .filter(|node| {
                matches!(
                    node.kind.as_str(),
                    "fe.rmir.statement" | "fe.rmir.terminator"
                )
            })
            .count();
        let call_count = graph
            .edges
            .iter()
            .filter(|edge| edge.role == EdgeRole::Call)
            .count();
        functions.push(LoweredRmirFunction {
            symbol,
            instance,
            lowered: LoweredRmir {
                graph_key,
                graph,
                function_count: 1,
                block_count,
                statement_count,
                call_count,
            },
        });
    }
    Ok(functions)
}

fn text_field<'a>(
    node: &'a riff_catalog_core::Node,
    dimension: Dimension,
    name: &str,
) -> Option<&'a str> {
    node.fields.iter().find_map(|field| {
        if field.dimension == dimension && field.name.as_str() == name {
            match &field.value {
                riff_catalog_core::Value::Text(value) => Some(value.as_str()),
                _ => None,
            }
        } else {
            None
        }
    })
}

fn include_direct_data_neighbors(
    source: &Graph,
    frontier: &BTreeSet<NodeKey>,
    selected: &mut BTreeSet<NodeKey>,
) {
    for edge in source
        .edges
        .iter()
        .filter(|edge| edge.role == EdgeRole::Data)
    {
        if frontier.contains(&edge.source) || frontier.contains(&edge.target) {
            selected.insert(edge.source.clone());
            selected.insert(edge.target.clone());
        }
    }
}

fn include_ancestry(source: &Graph, selected: &mut BTreeSet<NodeKey>) {
    let source_root = NodeKey::entity(source.graph_key.owner.clone());
    loop {
        let mut changed = false;
        for child in &source.children {
            if selected.contains(&child.child) && child.parent != source_root {
                changed |= selected.insert(child.parent.clone());
            }
        }
        if !changed {
            break;
        }
    }
}

fn project_selected_package(
    owner: &str,
    source: &Graph,
    selected: BTreeSet<NodeKey>,
    namespace: &str,
    variant: &str,
) -> Result<LoweredRmir, LowerError> {
    let projection_owner = EntityKey::new(namespace, owner, "package")?;
    let graph_key = GraphKey::new(projection_owner.clone(), variant)?;
    let root = NodeKey::entity(projection_owner);
    let source_root = NodeKey::entity(source.graph_key.owner.clone());
    let mut graph = Graph::new(graph_key.clone());
    graph.add_node(root.clone(), namespace)?;
    for key in &selected {
        let node = source
            .nodes
            .get(key)
            .expect("a projected node must remain in the source graph");
        graph.nodes.insert(key.clone(), node.clone());
    }
    for child in &source.children {
        if child.parent == source_root && selected.contains(&child.child) {
            graph.add_child(&root, child.label.as_str(), child.ordinal, &child.child)?;
        } else if selected.contains(&child.parent) && selected.contains(&child.child) {
            graph.children.push(child.clone());
        }
    }
    graph.edges.extend(
        source
            .edges
            .iter()
            .filter(|edge| selected.contains(&edge.source) && selected.contains(&edge.target))
            .cloned(),
    );
    graph.validate()?;

    let function_count = graph
        .nodes
        .values()
        .filter(|node| node.kind.as_str() == "fe.rmir.function")
        .count();
    let block_count = graph
        .nodes
        .values()
        .filter(|node| node.kind.as_str() == "fe.rmir.block")
        .count();
    let statement_count = graph
        .nodes
        .values()
        .filter(|node| {
            matches!(
                node.kind.as_str(),
                "fe.rmir.statement" | "fe.rmir.terminator"
            )
        })
        .count();
    let call_count = graph
        .edges
        .iter()
        .filter(|edge| edge.role == EdgeRole::Call)
        .count();
    Ok(LoweredRmir {
        graph_key,
        graph,
        function_count,
        block_count,
        statement_count,
        call_count,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const BODY: &str = r#"package fixture {
  primary_object: main
  root_objects: [main]
  const_regions: 0
  functions:
    helper (internal, inline=Auto, instance=helper-instance)
      fn helper-instance(
        %0: uint32
      ) -> uint32 {
        locals:
          %0: ty=u32, carrier=uint32, root=none
          %1: ty=u32, carrier=uint32, root=none
        bb0:
          [0] %1 = checked_Add %0, %0;
          -> return %1
      }
    main (internal, inline=Auto, instance=main-instance)
      fn main-instance(
      ) -> uint32 {
        locals:
          %0: ty=u32, carrier=uint32, root=none
          %1: ty=Pair, carrier=agg struct Pair, root=none
          %2: ty=u32, carrier=uint32, root=none
          %3: ty=u32, carrier=uint32, root=none
        bb0:
          [0] %0 = uint32(0x07);
          [1] %1 = aggregate_make struct Pair(%0, %0);
          [2] %2 = extract_value %1, 0;
          [3] %3 = call helper-instance(%2);
          -> branch %3 ? bb1 : bb1

        bb1:
          [0] %3 = checked_Mul %3, %0;
          -> return %3
      }
}"#;

    fn snapshot(body: &str) -> String {
        let digest = format!("{:x}", Sha256::digest(body.as_bytes()));
        format!("# fe-rmir-observation/1\n# phase: runtime-package\n# sha256: {digest}\n{body}\n")
    }

    fn operations(lowered: &LoweredRmir) -> BTreeSet<&str> {
        lowered.graph.nodes.values().filter_map(operation).collect()
    }

    #[test]
    fn lowers_typed_data_control_and_call_structure() {
        let (lowered, aggregate) =
            parse_and_lower_views("fixture", &snapshot(BODY)).expect("snapshot should lower");
        assert_eq!(lowered.function_count, 2);
        assert_eq!(lowered.block_count, 3);
        assert_eq!(lowered.statement_count, 9);
        assert_eq!(lowered.call_count, 1);
        assert!(
            lowered
                .graph
                .edges
                .iter()
                .any(|edge| edge.role == EdgeRole::Control)
        );
        assert!(
            lowered
                .graph
                .edges
                .iter()
                .any(|edge| edge.role == EdgeRole::Call)
        );
        assert!(structure_census(&lowered).is_ok());
        assert!(aggregate_structure_census(&aggregate).is_ok());
    }

    #[test]
    fn aggregate_projection_keeps_producers_without_unrelated_followup_work() {
        let (_, aggregate) =
            parse_and_lower_views("fixture", &snapshot(BODY)).expect("snapshot should lower");
        let operations = operations(&aggregate);
        assert!(operations.contains("const_scalar"));
        assert!(operations.contains("aggregate_make"));
        assert!(operations.contains("extract_value"));
        assert!(!operations.contains("checked_Mul"));
    }

    #[test]
    fn materialization_frontier_does_not_pull_transitive_producers() {
        let views = parse_and_lower_analysis_views("fixture", &snapshot(BODY))
            .expect("snapshot should lower");
        let operations = operations(&views.materialization);
        assert!(operations.contains("aggregate_make"));
        assert!(operations.contains("extract_value"));
        assert!(!operations.contains("const_scalar"));
        assert!(!operations.contains("call"));
        assert!(!operations.contains("checked_Mul"));
        assert!(materialization_structure_census(&views.materialization).is_ok());
    }

    #[test]
    fn call_frontier_keeps_sites_and_callees_without_function_bodies() {
        let views = parse_and_lower_analysis_views("fixture", &snapshot(BODY))
            .expect("snapshot should lower");
        let operations = operations(&views.calls);
        assert_eq!(views.calls.call_count, 1);
        assert!(operations.contains("call"));
        assert!(!operations.contains("checked_Add"));
        assert!(!operations.contains("checked_Mul"));
        assert!(views.calls.graph.nodes.values().any(|node| {
            node.kind.as_str() == "fe.rmir.function"
                && node.fields.iter().any(|field| {
                    field.dimension == Dimension::Names
                        && field.name.as_str() == "symbol"
                        && field.value == riff_catalog_core::Value::Text("helper".to_string())
                })
        }));
        assert!(call_structure_census(&views.calls).is_ok());
    }

    #[test]
    fn function_projection_keeps_one_body_and_fingerprints_direct_callees() {
        let lowered = parse_and_lower_analysis_views("fixture", &snapshot(BODY))
            .expect("snapshot should lower")
            .package;
        let functions =
            project_function_packages("fixture", &lowered).expect("function bodies should project");
        assert_eq!(functions.len(), 2);
        let helper = functions
            .iter()
            .find(|function| function.symbol == "helper")
            .expect("helper projection");
        assert_eq!(helper.lowered.block_count, 1);
        assert_eq!(helper.lowered.call_count, 0);
        assert!(operations(&helper.lowered).contains("checked_Add"));

        let main = functions
            .iter()
            .find(|function| function.symbol == "main")
            .expect("main projection");
        assert_eq!(main.lowered.block_count, 2);
        assert_eq!(main.lowered.call_count, 1);
        assert!(operations(&main.lowered).contains("checked_Mul"));
        assert!(!operations(&main.lowered).contains("checked_Add"));
        let callee = main
            .lowered
            .graph
            .nodes
            .values()
            .find(|node| {
                node.kind.as_str() == "fe.rmir.function"
                    && text_field(node, Dimension::Names, "symbol") == Some("helper")
            })
            .expect("main should retain a helper stub");
        assert!(
            text_field(callee, Dimension::Structure, "callee_body").is_some(),
            "the direct callee stub must carry its complete structural body digest"
        );
    }

    #[test]
    fn function_shapes_ignore_names_but_distinguish_operations() {
        let project_helper = |owner: &str, body: &str| {
            let lowered = parse_and_lower_analysis_views(owner, &snapshot(body))
                .expect("snapshot should lower")
                .package;
            project_function_packages(owner, &lowered)
                .expect("function bodies should project")
                .into_iter()
                .find(|function| function.symbol != "main")
                .expect("helper projection")
        };
        let original = project_helper("original", BODY);
        let renamed_body = BODY.replace("helper", "doubler");
        let renamed = project_helper("renamed", &renamed_body);
        let changed_body = BODY.replace("checked_Add", "checked_Mul");
        let changed = project_helper("changed", &changed_body);

        assert_eq!(
            function_graph_structure_digest(&original.lowered).unwrap(),
            function_graph_structure_digest(&renamed.lowered).unwrap()
        );
        assert_ne!(
            function_graph_structure_digest(&original.lowered).unwrap(),
            function_graph_structure_digest(&changed.lowered).unwrap()
        );
        let census = function_structure_census(&[original, renamed, changed]).unwrap();
        assert_eq!(census.functions, 3);
        assert_eq!(census.distinct_shapes, 2);
        assert_eq!(census.repeated_occurrences, 1);
        assert_eq!(census.largest_class, 2);
    }

    #[test]
    fn caller_fingerprints_retain_callee_constants() {
        let helper_with_constant = BODY.replacen("checked_Add %0, %0", "uint32(0x07)", 1);
        let changed_constant = helper_with_constant.replacen("uint32(0x07)", "uint32(0x08)", 1);
        let project_main = |owner: &str, body: &str| {
            let lowered = parse_and_lower_analysis_views(owner, &snapshot(body))
                .expect("snapshot should lower")
                .package;
            project_function_packages(owner, &lowered)
                .expect("function bodies should project")
                .into_iter()
                .find(|function| function.symbol == "main")
                .expect("main projection")
        };
        let left = project_main("left", &helper_with_constant);
        let right = project_main("right", &changed_constant);
        assert_eq!(
            function_graph_digests(&left.lowered, &[Dimension::Structure]).unwrap(),
            function_graph_digests(&right.lowered, &[Dimension::Structure]).unwrap(),
            "constant-only changes remain outside the structure facet"
        );
        assert_ne!(
            function_graph_digests(&left.lowered, &[Dimension::Structure, Dimension::Constants])
                .unwrap(),
            function_graph_digests(
                &right.lowered,
                &[Dimension::Structure, Dimension::Constants]
            )
            .unwrap(),
            "a caller must inherit its direct callee's constant distinction"
        );
    }

    #[test]
    fn checksum_mutation_fails_closed() {
        let source = snapshot(BODY).replace("uint32(0x07)", "uint32(0x08)");
        assert!(matches!(
            parse_and_lower_views("fixture", &source),
            Err(LowerError::DigestMismatch { .. })
        ));
    }

    #[test]
    fn constants_move_the_constants_facet_without_moving_structure() {
        let left = parse_and_lower_views("left", &snapshot(BODY))
            .expect("left should lower")
            .0;
        let right_body = BODY.replace("uint32(0x07)", "uint32(0x08)");
        let right = parse_and_lower_views("right", &snapshot(&right_body))
            .expect("right should lower")
            .0;
        let digest = |lowered: &LoweredRmir, dimensions: &[Dimension]| {
            let policy = HashPolicy::new(
                FE_RMIR_LEVEL,
                ViewMode::AnonymousShape,
                CyclePolicy::CondenseScc,
            )
            .unwrap();
            digest_graph(
                &DigestRequest::new(
                    lowered.graph_key.clone(),
                    policy,
                    dimensions.iter().copied(),
                )
                .unwrap(),
                &lowered.graph,
            )
            .unwrap()
            .hashes
            .graph
            .values
        };
        assert_eq!(
            digest(&left, &[Dimension::Structure]),
            digest(&right, &[Dimension::Structure])
        );
        assert_ne!(
            digest(&left, &[Dimension::Constants]),
            digest(&right, &[Dimension::Constants])
        );
    }
}
