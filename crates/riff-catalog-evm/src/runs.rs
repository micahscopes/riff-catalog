//! Repeated instruction runs in EVM bytecode, compared with jump-label
//! relocation, and a non-overlapping selection of them for byte accounting.
//!
//! ## What a run key keeps
//!
//! A run is a sequence of consecutive decoded instructions inside one scope
//! (typically one emitted function). Two runs match under
//! [`RUN_POLICY`] when:
//!
//! - every opcode is the same, in order;
//! - every PUSH immediate that is not a code label is the same value;
//! - every code-label PUSH that targets a JUMPDEST inside the run targets the
//!   same offset relative to the run start (so a relocated copy still matches);
//! - code-label PUSHes that target code outside the run are replaced by ports
//!   numbered by first use. Reusing one outside target twice reuses its port,
//!   so the equality pattern among outside targets is kept. The actual targets
//!   are returned per occurrence as bindings, and a class reports how many of
//!   its ports differ between copies.
//!
//! The EVM is a stack machine: DUP/SWAP/POP are explicit instructions, so equal
//! instruction sequences also have equal internal dataflow wiring, given the
//! same stack at entry. The key therefore includes the wiring inside the run.
//! What it does not include is anything about the stack or memory at entry.
//!
//! ## Label detection is a stated heuristic
//!
//! Bytecode does not mark which PUSH values are code addresses. This module
//! treats a PUSH1..PUSH4 as a label when its value is the pc of a decoded
//! JUMPDEST. A plain constant that happens to equal a JUMPDEST pc is then
//! treated as a label: if it points inside the run it must match by relative
//! offset (stricter than a value comparison, so it can only split classes), and
//! if it points outside it becomes a port whose value is reported as a binding.
//!
//! ## Selection
//!
//! Candidate classes come from maximal repeats (suffix array plus LCP intervals)
//! over a coarse token string in which every label reads alike, then are
//! partitioned by the exact key above. Selection is greedy by covered bytes
//! (copies times bytes per copy): the best class takes its non-overlapping
//! occurrences, those bytes are claimed, and every other class is re-evaluated
//! on unclaimed bytes only. Selected classes therefore never overlap and their
//! covered bytes add up. Greedy selection is not an optimal cover, and a
//! partially claimed occurrence is dropped rather than trimmed.
//!
//! ## Classes are core facet addresses
//!
//! Each copy is lowered into a core graph ([`run_graph`], level [`RUN_LEVEL`])
//! and its class is its core facet address: Structure plus Constants for the
//! exact key ([`RUN_POLICY`]), Structure only for the constants-blind key
//! ([`RUN_POLICY_CONSTANT_PORTS`], `constants_as_ports`). The suffix array and
//! the per-occurrence token key only propose candidate groups quickly; the
//! core address decides them. With the constants-blind facet, non-label PUSH
//! values are also returned as bindings, in first-use order.
//!
//! A match is structural correspondence under this key. It is not a proof that
//! the copies behave the same or that sharing them is safe or smaller.

use std::collections::{BTreeMap, BinaryHeap, HashMap};

use riff_catalog_core::{
    CyclePolicy, DigestRequest, Dimension, EntityKey, Facet, Graph, GraphKey, HashPolicy, NodeKey,
    ViewMode, digest_graph,
};

/// Versioned lowering of one run into a core graph ([`run_graph`]). Run
/// classes are equal core facet addresses of these graphs.
pub const RUN_LEVEL: &str = "evm-run/1";
/// Human-readable name of the exact facet: Structure plus Constants.
pub const RUN_POLICY: &str = "evm-run/1 facet structure+constants";
/// Human-readable name of the constants-blind facet: Structure only. It
/// groups copies that differ only in PUSH values, such as the same error path
/// with different selectors: code that a parameter could share, not code that
/// is identical. The equality pattern among constants is Structure (see
/// [`run_graph`]), so a class still needs one parameter per distinct value.
pub const RUN_POLICY_CONSTANT_PORTS: &str = "evm-run/1 facet structure";

/// The hash policy of [`RUN_LEVEL`] graphs: anonymous shape (pcs and node keys
/// never enter a digest), acyclic (a run graph is a sequence).
pub fn run_hash_policy() -> HashPolicy {
    HashPolicy::new(RUN_LEVEL, ViewMode::AnonymousShape, CyclePolicy::Reject)
        .expect("static run policy is valid")
}

/// The core facet a run census compares on: Structure plus Constants
/// (exact), or Structure only (constants blind).
pub fn run_facet(constants_blind: bool) -> Facet {
    let policy_id = run_hash_policy().policy_id();
    if constants_blind {
        Facet::structure_only(policy_id)
    } else {
        Facet::new(policy_id, [Dimension::Structure, Dimension::Constants])
            .expect("non-empty dimensions")
    }
}

const JUMPDEST: u8 = 0x5b;
/// Default cap on label-token visits while partitioning candidates.
pub const DEFAULT_WORK_BUDGET: u64 = 400_000_000;

/// One decoded instruction: its pc, total byte length and opcode.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Instruction {
    pub pc: u32,
    pub len: u32,
    pub opcode: u8,
}

/// Linear-sweep decode. A truncated trailing PUSH is clamped to the code end.
pub fn decode(code: &[u8]) -> Vec<Instruction> {
    let mut out = Vec::new();
    let mut pc = 0usize;
    while pc < code.len() {
        let opcode = code[pc];
        let len = (1 + push_len(opcode)).min(code.len() - pc);
        out.push(Instruction {
            pc: pc as u32,
            len: len as u32,
            opcode,
        });
        pc += len;
    }
    out
}

fn push_len(opcode: u8) -> usize {
    if (0x60..=0x7f).contains(&opcode) {
        (opcode - 0x5f) as usize
    } else {
        0
    }
}

/// A half-open pc range that occurrences must stay inside.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct Scope {
    pub start: u32,
    pub end: u32,
}

#[derive(Clone, Copy, Debug)]
pub struct RunCensusOptions {
    /// Ignore runs shorter than this many bytes per copy.
    pub min_run_bytes: u32,
    /// Upper bound on label-token visits during partitioning; exceeding it is
    /// an error, never a silently partial answer.
    pub work_budget: u64,
    /// Compare non-label PUSH values as ports ([`RUN_POLICY_CONSTANT_PORTS`])
    /// instead of exactly ([`RUN_POLICY`]).
    pub constants_as_ports: bool,
    /// Ignore runs of fewer instructions than this. A single instruction is
    /// not a shape (with constant ports, every PUSH32 would match every other).
    pub min_run_instructions: u32,
    /// With constant ports, drop candidate classes in which more than this
    /// many constant ports take different values across the copies, so a
    /// class is code that at most this many parameters could share.
    pub max_varying_constants: Option<usize>,
}

impl Default for RunCensusOptions {
    fn default() -> Self {
        Self {
            min_run_bytes: 32,
            work_budget: DEFAULT_WORK_BUDGET,
            constants_as_ports: false,
            min_run_instructions: 2,
            max_varying_constants: None,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RunOccurrence {
    /// Half-open pc range.
    pub start: u32,
    pub end: u32,
    /// Actual outside label targets, in port order.
    pub bindings: Vec<u32>,
    /// Actual constant values (hex), in constant-port order. Empty unless
    /// constants are compared as ports.
    pub constant_bindings: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RunClass {
    /// Core facet address (hex) of the class's run graph ([`run_graph`]).
    pub digest: String,
    pub bytes_per_copy: u32,
    pub instructions_per_copy: u32,
    /// Selected non-overlapping occurrences, in pc order.
    pub occurrences: Vec<RunOccurrence>,
    /// Number of ports (distinct outside label targets per copy).
    pub ports: usize,
    /// Ports whose bound target is not the same in every selected copy.
    pub varying_ports: usize,
    /// Constant ports, and those whose value differs between copies.
    pub constant_ports: usize,
    pub varying_constant_ports: usize,
}

impl RunClass {
    pub fn covered_bytes(&self) -> u64 {
        self.bytes_per_copy as u64 * self.occurrences.len() as u64
    }

    /// Bytes beyond the first copy. Not a savings estimate: sharing has its own
    /// call, return and parameter costs.
    pub fn extra_bytes(&self) -> u64 {
        self.bytes_per_copy as u64 * (self.occurrences.len() as u64).saturating_sub(1)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RunCensus {
    /// Name of the facet the classes were compared on.
    pub policy: &'static str,
    /// The core facet id (hex) behind `policy`.
    pub facet_id: String,
    pub min_run_bytes: u32,
    /// Selected classes, in selection order (descending covered bytes).
    pub classes: Vec<RunClass>,
    /// Exact-key candidate classes considered before selection.
    pub candidate_classes: usize,
    /// Bytes of code inside the given scopes.
    pub scoped_bytes: u64,
    /// Sum of selected classes' covered bytes (they never overlap).
    pub selected_bytes: u64,
}

#[derive(Debug, PartialEq, Eq)]
pub enum RunCensusError {
    ScopeOutOfBounds { start: u32, end: u32, len: usize },
    OverlappingScopes { first: Scope, second: Scope },
    WorkBudgetExceeded { budget: u64 },
}

impl std::fmt::Display for RunCensusError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::ScopeOutOfBounds { start, end, len } => {
                write!(f, "scope {start}..{end} is outside the {len}-byte code")
            }
            Self::OverlappingScopes { first, second } => write!(
                f,
                "scopes {}..{} and {}..{} overlap",
                first.start, first.end, second.start, second.end
            ),
            Self::WorkBudgetExceeded { budget } => {
                write!(
                    f,
                    "run census exceeded its work budget of {budget} label visits"
                )
            }
        }
    }
}

impl std::error::Error for RunCensusError {}

/// The token stream: decoded instructions inside scopes, one unique sentinel
/// after each scope so no repeat can cross a scope boundary.
struct Stream {
    /// Coarse token id per position; sentinels are unique ids above the alphabet.
    tokens: Vec<u32>,
    /// Instruction per position (`None` for sentinels).
    insts: Vec<Option<Instruction>>,
    /// Prefix sums of byte lengths over positions.
    byte_prefix: Vec<u64>,
    /// Label target per position, if the instruction is a code-label PUSH.
    label: Vec<Option<u32>>,
    /// Positions holding labels, ascending.
    label_positions: Vec<usize>,
    /// Immediate bytes per position, for non-label PUSHes compared as ports.
    constant: Vec<Option<Vec<u8>>>,
    /// Positions whose key needs per-occurrence encoding (labels, and
    /// constants compared as ports), ascending.
    special_positions: Vec<usize>,
}

fn build_stream(code: &[u8], scopes: &[Scope], constants_as_ports: bool) -> Stream {
    let insts = decode(code);
    let jumpdests: std::collections::HashSet<u32> = insts
        .iter()
        .filter(|i| i.opcode == JUMPDEST)
        .map(|i| i.pc)
        .collect();
    let mut interned: HashMap<Vec<u8>, u32> = HashMap::new();
    let mut stream = Stream {
        tokens: Vec::new(),
        insts: Vec::new(),
        byte_prefix: vec![0],
        label: Vec::new(),
        label_positions: Vec::new(),
        constant: Vec::new(),
        special_positions: Vec::new(),
    };
    let mut raw_tokens: Vec<Option<u32>> = Vec::new();
    let mut cursor = 0usize;
    for scope in scopes {
        while cursor < insts.len() && insts[cursor].pc < scope.start {
            cursor += 1;
        }
        while cursor < insts.len() && insts[cursor].pc + insts[cursor].len <= scope.end {
            let inst = insts[cursor];
            let bytes = &code[inst.pc as usize..(inst.pc + inst.len) as usize];
            let n = push_len(inst.opcode);
            let target = (1..=4).contains(&n).then(|| {
                bytes[1..]
                    .iter()
                    .fold(0u64, |acc, b| (acc << 8) | u64::from(*b))
            });
            let target = target
                .filter(|v| bytes.len() == 1 + n && *v <= u64::from(u32::MAX))
                .map(|v| v as u32)
                .filter(|v| jumpdests.contains(v));
            let as_port = constants_as_ports && target.is_none() && bytes.len() > 1;
            let key = if target.is_some() {
                vec![inst.opcode]
            } else if as_port {
                // The value is compared per occurrence; the policies never mix
                // in one stream, so this marker cannot meet an exact key.
                vec![inst.opcode, 0xfe]
            } else {
                // Non-label: opcode plus exact immediate bytes. A non-label PUSH
                // key is always longer than one byte, so it never equals a label.
                bytes.to_vec()
            };
            let next = interned.len() as u32;
            let id = *interned.entry(key).or_insert(next);
            if target.is_some() {
                stream.label_positions.push(stream.insts.len());
            }
            if target.is_some() || as_port {
                stream.special_positions.push(stream.insts.len());
            }
            raw_tokens.push(Some(id));
            stream.insts.push(Some(inst));
            stream.label.push(target);
            stream.constant.push(as_port.then(|| bytes[1..].to_vec()));
            let last = *stream.byte_prefix.last().unwrap();
            stream.byte_prefix.push(last + u64::from(inst.len));
            cursor += 1;
        }
        raw_tokens.push(None);
        stream.insts.push(None);
        stream.label.push(None);
        stream.constant.push(None);
        let last = *stream.byte_prefix.last().unwrap();
        stream.byte_prefix.push(last);
    }
    let alphabet = interned.len() as u32;
    let mut sentinel = alphabet;
    stream.tokens = raw_tokens
        .into_iter()
        .map(|t| {
            t.unwrap_or_else(|| {
                sentinel += 1;
                sentinel
            })
        })
        .collect();
    stream
}

/// Prefix-doubling suffix array: O(n log^2 n), adequate for contract-sized code.
fn suffix_array(s: &[u32]) -> Vec<usize> {
    let n = s.len();
    let mut sa: Vec<usize> = (0..n).collect();
    let mut rank: Vec<u64> = s.iter().map(|&c| u64::from(c)).collect();
    let mut tmp = vec![0u64; n];
    let mut k = 1usize;
    if n <= 1 {
        return sa;
    }
    loop {
        let key = |i: usize, rank: &[u64]| (rank[i], if i + k < n { rank[i + k] + 1 } else { 0 });
        sa.sort_unstable_by_key(|&i| key(i, &rank));
        tmp[sa[0]] = 0;
        for w in 1..n {
            tmp[sa[w]] = tmp[sa[w - 1]] + u64::from(key(sa[w - 1], &rank) < key(sa[w], &rank));
        }
        std::mem::swap(&mut rank, &mut tmp);
        if rank[sa[n - 1]] as usize == n - 1 {
            break;
        }
        k *= 2;
    }
    sa
}

/// Kasai LCP: `lcp[i]` is the common prefix of suffixes `sa[i-1]` and `sa[i]`.
fn lcp_array(s: &[u32], sa: &[usize]) -> Vec<usize> {
    let n = s.len();
    let mut rank = vec![0usize; n];
    for (i, &p) in sa.iter().enumerate() {
        rank[p] = i;
    }
    let mut lcp = vec![0usize; n];
    let mut h = 0usize;
    for i in 0..n {
        if rank[i] > 0 {
            let j = sa[rank[i] - 1];
            while i + h < n && j + h < n && s[i + h] == s[j + h] {
                h += 1;
            }
            lcp[rank[i]] = h;
            h = h.saturating_sub(1);
        } else {
            h = 0;
        }
    }
    lcp
}

struct Candidate {
    positions: Vec<usize>,
    len: usize,
    bytes: u32,
    /// Core facet address shared by every position.
    address: riff_catalog_core::Digest,
}

/// Per-occurrence key: internal labels as relative offsets, outside labels
/// as first-use ports and, when compared as ports, constants as first-use
/// ports. Returns the key and the label and constant bindings.
struct OccurrenceKey {
    key: Vec<u64>,
    bindings: Vec<u32>,
    constant_bindings: Vec<Vec<u8>>,
}

fn occurrence_key(stream: &Stream, position: usize, len: usize, visits: &mut u64) -> OccurrenceKey {
    let start_pc = stream.insts[position].unwrap().pc;
    let end_pc =
        start_pc + (stream.byte_prefix[position + len] - stream.byte_prefix[position]) as u32;
    let first = stream.special_positions.partition_point(|&p| p < position);
    let mut out = OccurrenceKey {
        key: Vec::new(),
        bindings: Vec::new(),
        constant_bindings: Vec::new(),
    };
    let mut ports: BTreeMap<u32, u64> = BTreeMap::new();
    let mut constants: BTreeMap<&[u8], u64> = BTreeMap::new();
    for &p in &stream.special_positions[first..] {
        if p >= position + len {
            break;
        }
        *visits += 1;
        let offset = (p - position) as u64;
        let encoded = if let Some(target) = stream.label[p] {
            if (start_pc..end_pc).contains(&target) {
                u64::from(target - start_pc) << 2
            } else {
                let next = ports.len() as u64;
                let port = *ports.entry(target).or_insert_with(|| {
                    out.bindings.push(target);
                    next
                });
                (port << 2) | 1
            }
        } else {
            let value = stream.constant[p].as_deref().expect("special position");
            let next = constants.len() as u64;
            let port = *constants.entry(value).or_insert_with(|| {
                out.constant_bindings.push(value.to_vec());
                next
            });
            (port << 2) | 2
        };
        out.key.push(offset);
        out.key.push(encoded);
    }
    out
}

/// Lower one run into a core graph at [`RUN_LEVEL`]: an `evm.run` root whose
/// ordered `instruction` children are `evm.instruction` nodes. Each run is
/// given with, per instruction, its code-label target (if it is a label PUSH).
///
/// - Structure: `opcode`; for a label PUSH, `internal_target` (byte offset of a
///   target inside the run) or `outside_port` (port numbered by first use; the
///   target itself is a binding, not a field); for any other PUSH with an
///   immediate, `constant_port` (first-use number over the run's distinct
///   values, so the equality pattern among constants is structure).
/// - Constants: that PUSH's `immediate` bytes.
///
/// Structure plus Constants is the exact run key; Structure alone forgets the
/// values but keeps which constants are equal.
pub fn run_graph(code: &[u8], run: &[(Instruction, Option<u32>)]) -> Graph {
    let build = || -> Result<Graph, riff_catalog_core::CatalogError> {
        let owner = EntityKey::new("evm.run", "run", "root")?;
        let mut graph = Graph::new(GraphKey::new(owner.clone(), "run")?);
        let root = NodeKey::entity(owner.clone());
        graph.add_node(root.clone(), "evm.run")?;
        let Some((first, _)) = run.first() else {
            return Ok(graph);
        };
        let start_pc = first.pc;
        let end_pc = run.last().map_or(start_pc, |(i, _)| i.pc + i.len);
        let mut outside: BTreeMap<u32, u64> = BTreeMap::new();
        let mut constants: BTreeMap<&[u8], u64> = BTreeMap::new();
        for (k, (inst, label)) in run.iter().enumerate() {
            let node = NodeKey::derived(owner.clone(), k.to_string())?;
            graph.add_node(node.clone(), "evm.instruction")?;
            graph.add_field(
                &node,
                Dimension::Structure,
                "opcode",
                u64::from(inst.opcode),
            )?;
            let bytes = &code[inst.pc as usize..(inst.pc + inst.len) as usize];
            if let Some(target) = label {
                if (start_pc..end_pc).contains(target) {
                    graph.add_field(
                        &node,
                        Dimension::Structure,
                        "internal_target",
                        u64::from(target - start_pc),
                    )?;
                } else {
                    let next = outside.len() as u64;
                    let port = *outside.entry(*target).or_insert(next);
                    graph.add_field(&node, Dimension::Structure, "outside_port", port)?;
                }
            } else if bytes.len() > 1 {
                let value = &bytes[1..];
                let next = constants.len() as u64;
                let port = *constants.entry(value).or_insert(next);
                graph.add_field(&node, Dimension::Structure, "constant_port", port)?;
                graph.add_field(&node, Dimension::Constants, "immediate", value.to_vec())?;
            }
            graph.add_child(&root, "instruction", k as u32, &node)?;
        }
        Ok(graph)
    };
    build().expect("run graph keys and fields are well formed")
}

/// The address of a run graph at `facet`: equal addresses are one class.
pub fn run_address(graph: &Graph, facet: &Facet) -> riff_catalog_core::Digest {
    let request = DigestRequest::new(
        graph.graph_key.clone(),
        run_hash_policy(),
        facet.dimensions.iter().copied(),
    )
    .expect("facet has dimensions");
    digest_graph(&request, graph)
        .expect("run graphs are acyclic trees")
        .hashes
        .facet_address(facet)
        .expect("facet matches the run policy")
        .address_digest()
}

fn stream_run(stream: &Stream, position: usize, len: usize) -> Vec<(Instruction, Option<u32>)> {
    (position..position + len)
        .map(|p| {
            (
                stream.insts[p].expect("runs never include sentinels"),
                stream.label[p],
            )
        })
        .collect()
}

fn stream_address(
    code: &[u8],
    stream: &Stream,
    position: usize,
    len: usize,
    facet: &Facet,
) -> riff_catalog_core::Digest {
    run_address(&run_graph(code, &stream_run(stream, position, len)), facet)
}

/// Find repeated runs and select a non-overlapping set of them.
///
/// `scopes` must be sorted-compatible, non-overlapping and inside the code.
/// Instructions not wholly inside a scope are ignored.
pub fn census_runs(
    code: &[u8],
    scopes: &[Scope],
    options: RunCensusOptions,
) -> Result<RunCensus, RunCensusError> {
    let mut scopes = scopes.to_vec();
    scopes.sort();
    for scope in &scopes {
        if scope.start > scope.end || scope.end as usize > code.len() {
            return Err(RunCensusError::ScopeOutOfBounds {
                start: scope.start,
                end: scope.end,
                len: code.len(),
            });
        }
    }
    for pair in scopes.windows(2) {
        if pair[1].start < pair[0].end {
            return Err(RunCensusError::OverlappingScopes {
                first: pair[0],
                second: pair[1],
            });
        }
    }
    let stream = build_stream(code, &scopes, options.constants_as_ports);
    let policy = if options.constants_as_ports {
        RUN_POLICY_CONSTANT_PORTS
    } else {
        RUN_POLICY
    };
    let facet = run_facet(options.constants_as_ports);
    let scoped_bytes = *stream.byte_prefix.last().unwrap();
    let n = stream.tokens.len();
    let sa = suffix_array(&stream.tokens);
    let lcp = lcp_array(&stream.tokens, &sa);

    // Enumerate LCP intervals: (common length, suffix-array range).
    let mut intervals: Vec<(usize, usize, usize)> = Vec::new();
    let mut stack: Vec<(usize, usize)> = vec![(0, 0)];
    for i in 1..=n {
        let current = lcp.get(i).copied().unwrap_or(0);
        let mut lb = i - 1;
        while current < stack.last().unwrap().0 {
            let (l, left) = stack.pop().unwrap();
            intervals.push((l, left, i - 1));
            lb = left;
        }
        if current > stack.last().unwrap().0 {
            stack.push((current, lb));
        }
    }

    let mut visits = 0u64;
    let mut candidates: Vec<Candidate> = Vec::new();
    for (len, left, right) in intervals {
        let first = sa[left];
        let bytes = (stream.byte_prefix[first + len] - stream.byte_prefix[first]) as u32;
        if bytes < options.min_run_bytes.max(1) || (len as u32) < options.min_run_instructions {
            continue;
        }
        let mut groups: HashMap<Vec<u64>, Vec<usize>> = HashMap::new();
        for &position in &sa[left..=right] {
            let key = occurrence_key(&stream, position, len, &mut visits).key;
            if visits > options.work_budget {
                return Err(RunCensusError::WorkBudgetExceeded {
                    budget: options.work_budget,
                });
            }
            groups.entry(key).or_default().push(position);
        }
        // The fast key above only proposes groups; the core facet address of
        // each copy's run graph decides the classes.
        let mut classes_by_address: BTreeMap<riff_catalog_core::Digest, Vec<usize>> =
            BTreeMap::new();
        for (_, positions) in groups {
            if positions.len() < 2 {
                continue;
            }
            for position in positions {
                classes_by_address
                    .entry(stream_address(code, &stream, position, len, &facet))
                    .or_default()
                    .push(position);
            }
        }
        for (address, mut positions) in classes_by_address {
            if positions.len() < 2 {
                continue;
            }
            if let Some(max) = options.max_varying_constants {
                let bound: Vec<Vec<Vec<u8>>> = positions
                    .iter()
                    .map(|&p| occurrence_key(&stream, p, len, &mut visits).constant_bindings)
                    .collect();
                let varying = (0..bound[0].len())
                    .filter(|&k| bound.iter().any(|b| b[k] != bound[0][k]))
                    .count();
                if varying > max {
                    continue;
                }
            }
            positions.sort_unstable();
            candidates.push(Candidate {
                positions,
                len,
                bytes,
                address,
            });
        }
    }
    let candidate_classes = candidates.len();

    // Greedy selection on unclaimed bytes, re-evaluated lazily.
    let mut claimed = vec![false; code.len()];
    let pc_of = |p: usize| stream.insts[p].unwrap().pc;
    let available = |c: &Candidate, claimed: &[bool]| -> Vec<usize> {
        let mut out = Vec::new();
        let mut last_end = 0u32;
        for &p in &c.positions {
            let start = pc_of(p);
            let end = start + c.bytes;
            if (start >= last_end || out.is_empty())
                && !claimed[start as usize..end as usize].iter().any(|b| *b)
            {
                out.push(p);
                last_end = end;
            }
        }
        out
    };
    let mut heap: BinaryHeap<(u64, std::cmp::Reverse<usize>)> = candidates
        .iter()
        .enumerate()
        .map(|(i, c)| {
            (
                u64::from(c.bytes) * c.positions.len() as u64,
                std::cmp::Reverse(i),
            )
        })
        .collect();
    let mut classes = Vec::new();
    let mut selected_bytes = 0u64;
    while let Some((value, std::cmp::Reverse(index))) = heap.pop() {
        let candidate = &candidates[index];
        let positions = available(candidate, &claimed);
        let now = u64::from(candidate.bytes) * positions.len() as u64;
        if positions.len() < 2 {
            continue;
        }
        if now < value {
            heap.push((now, std::cmp::Reverse(index)));
            continue;
        }
        let mut occurrences = Vec::new();
        for &p in &positions {
            let bound = occurrence_key(&stream, p, candidate.len, &mut visits);
            let start = pc_of(p);
            let end = start + candidate.bytes;
            claimed[start as usize..end as usize].fill(true);
            occurrences.push(RunOccurrence {
                start,
                end,
                bindings: bound.bindings,
                constant_bindings: bound
                    .constant_bindings
                    .iter()
                    .map(|v| v.iter().map(|b| format!("{b:02x}")).collect())
                    .collect(),
            });
        }
        let ports = occurrences[0].bindings.len();
        let varying_ports = (0..ports)
            .filter(|&k| {
                occurrences
                    .iter()
                    .any(|o| o.bindings[k] != occurrences[0].bindings[k])
            })
            .count();
        let constant_ports = occurrences[0].constant_bindings.len();
        let varying_constant_ports = (0..constant_ports)
            .filter(|&k| {
                occurrences
                    .iter()
                    .any(|o| o.constant_bindings[k] != occurrences[0].constant_bindings[k])
            })
            .count();
        classes.push(RunClass {
            digest: candidate.address.to_hex(),
            bytes_per_copy: candidate.bytes,
            instructions_per_copy: candidate.len as u32,
            occurrences,
            ports,
            varying_ports,
            constant_ports,
            varying_constant_ports,
        });
        selected_bytes += now;
    }
    Ok(RunCensus {
        policy,
        facet_id: facet.facet_id().to_hex(),
        min_run_bytes: options.min_run_bytes,
        classes,
        candidate_classes,
        scoped_bytes,
        selected_bytes,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const PUSH1: u8 = 0x60;
    const PUSH2: u8 = 0x61;
    const JUMP: u8 = 0x56;
    const ADD: u8 = 0x01;
    const MUL: u8 = 0x02;
    const DUP1: u8 = 0x80;
    const SWAP1: u8 = 0x90;
    const POP: u8 = 0x50;
    const STOP: u8 = 0x00;

    fn whole(code: &[u8]) -> Vec<Scope> {
        vec![Scope {
            start: 0,
            end: code.len() as u32,
        }]
    }

    fn body(filler: u8) -> Vec<u8> {
        // 12 bytes of straight-line code.
        vec![
            PUSH1, filler, DUP1, ADD, SWAP1, POP, PUSH1, 0x07, MUL, DUP1, ADD, POP,
        ]
    }

    fn options(min: u32) -> RunCensusOptions {
        RunCensusOptions {
            min_run_bytes: min,
            ..RunCensusOptions::default()
        }
    }

    #[test]
    fn decode_clamps_truncated_push() {
        let insts = decode(&[PUSH2, 0x01]);
        assert_eq!(
            insts,
            vec![Instruction {
                pc: 0,
                len: 2,
                opcode: PUSH2
            }]
        );
    }

    #[test]
    fn identical_copies_form_one_class_and_bytes_add_up() {
        let mut code = body(1);
        code.push(STOP);
        code.extend(body(1));
        code.push(STOP);
        code.extend(body(1));
        let census = census_runs(&code, &whole(&code), options(8)).unwrap();
        assert_eq!(census.classes.len(), 1);
        let class = &census.classes[0];
        assert_eq!(class.occurrences.len(), 3);
        assert!(class.bytes_per_copy >= 12);
        assert_eq!(census.selected_bytes, class.covered_bytes());
        assert_eq!(class.ports, 0);
    }

    #[test]
    fn different_constants_do_not_match() {
        let mut code = body(1);
        code.push(STOP);
        code.extend(body(2));
        let census = census_runs(&code, &whole(&code), options(12)).unwrap();
        assert!(census.classes.is_empty(), "{census:?}");
    }

    #[test]
    fn different_wiring_does_not_match() {
        // Same opcodes multiset, different stack wiring (SWAP1 moved).
        let a = vec![
            PUSH1, 1, PUSH1, 2, SWAP1, DUP1, ADD, POP, PUSH1, 3, MUL, POP,
        ];
        let b = vec![
            PUSH1, 1, PUSH1, 2, DUP1, SWAP1, ADD, POP, PUSH1, 3, MUL, POP,
        ];
        let mut code = a;
        code.push(STOP);
        code.extend(b);
        let census = census_runs(&code, &whole(&code), options(12)).unwrap();
        assert!(census.classes.is_empty());
    }

    /// A copy that jumps to its own internal JUMPDEST: relocated copies match
    /// because internal labels compare by relative offset.
    fn relocatable_copy(at: u16) -> Vec<u8> {
        let target = at + 4;
        let [hi, lo] = target.to_be_bytes();
        let mut v = vec![PUSH2, hi, lo, JUMP, JUMPDEST];
        v.extend(body(9));
        v
    }

    #[test]
    fn relocated_internal_labels_match() {
        let mut code = relocatable_copy(0);
        let second = code.len() as u16 + 1;
        code.push(STOP);
        code.extend(relocatable_copy(second));
        let census = census_runs(&code, &whole(&code), options(16)).unwrap();
        assert_eq!(census.classes.len(), 1, "{census:?}");
        let class = &census.classes[0];
        assert_eq!(class.occurrences.len(), 2);
        assert_eq!(class.occurrences[0].start, 0);
        assert_eq!(class.occurrences[1].start, u32::from(second));
        assert_eq!(class.ports, 0);
    }

    #[test]
    fn outside_targets_become_ports_with_bindings() {
        // Two shared tails at the end; each copy jumps to a different one.
        let copy = |tail: u16| {
            let [hi, lo] = tail.to_be_bytes();
            let mut v = body(5);
            v.extend([PUSH2, hi, lo, JUMP]);
            v
        };
        let mut code = copy(0);
        code.push(STOP);
        code.extend(copy(0));
        let tail_a = code.len() as u16;
        code.extend([JUMPDEST, STOP, JUMPDEST, STOP]);
        let tail_b = tail_a + 2;
        // Patch targets: first copy -> tail_a, second -> tail_b.
        let first_push = 12;
        code[first_push + 1..first_push + 3].copy_from_slice(&tail_a.to_be_bytes());
        let second_push = 16 + 1 + 12;
        code[second_push + 1..second_push + 3].copy_from_slice(&tail_b.to_be_bytes());
        let scopes = vec![Scope {
            start: 0,
            end: u32::from(tail_a),
        }];
        let census = census_runs(&code, &scopes, options(16)).unwrap();
        assert_eq!(census.classes.len(), 1, "{census:?}");
        let class = &census.classes[0];
        assert_eq!(class.ports, 1);
        assert_eq!(class.varying_ports, 1);
        assert_eq!(class.occurrences[0].bindings, vec![u32::from(tail_a)]);
        assert_eq!(class.occurrences[1].bindings, vec![u32::from(tail_b)]);
    }

    #[test]
    fn runs_never_cross_scope_boundaries() {
        let mut code = body(1);
        code.extend(body(1));
        let half = body(1).len() as u32;
        // One scope per copy of the pair halves: [a|a] and nothing crossing.
        let scopes = vec![
            Scope {
                start: 0,
                end: half,
            },
            Scope {
                start: half,
                end: 2 * half,
            },
        ];
        let census = census_runs(&code, &scopes, options(8)).unwrap();
        for class in &census.classes {
            for o in &class.occurrences {
                assert!(o.end <= half || o.start >= half, "{o:?} crosses a scope");
            }
        }
        assert_eq!(census.classes.len(), 1);
    }

    #[test]
    fn selected_classes_never_overlap() {
        // A long repeat containing a short one that also repeats elsewhere.
        let short = vec![PUSH1, 0x44, DUP1, MUL, POP, PUSH1, 0x45, ADD, POP];
        let mut long = short.clone();
        long.extend(body(3));
        let mut code = Vec::new();
        for _ in 0..2 {
            code.extend(&long);
            code.push(STOP);
        }
        for _ in 0..3 {
            code.extend(&short);
            code.push(STOP);
        }
        let census = census_runs(&code, &whole(&code), options(8)).unwrap();
        let mut covered = vec![0u8; code.len()];
        for class in &census.classes {
            for o in &class.occurrences {
                for b in &mut covered[o.start as usize..o.end as usize] {
                    *b += 1;
                }
            }
        }
        assert!(covered.iter().all(|c| *c <= 1));
        let total: u64 = covered.iter().map(|c| u64::from(*c)).sum();
        assert_eq!(total, census.selected_bytes);
    }

    #[test]
    fn overlapping_scopes_are_rejected() {
        let code = body(1);
        let scopes = vec![Scope { start: 0, end: 8 }, Scope { start: 4, end: 12 }];
        assert!(matches!(
            census_runs(&code, &scopes, options(4)),
            Err(RunCensusError::OverlappingScopes { .. })
        ));
    }

    #[test]
    fn work_budget_is_explicit() {
        let mut code = Vec::new();
        for _ in 0..4 {
            code.extend(relocatable_copy(code.len() as u16));
        }
        let result = census_runs(
            &code,
            &whole(&code),
            RunCensusOptions {
                min_run_bytes: 4,
                work_budget: 1,
                ..RunCensusOptions::default()
            },
        );
        assert_eq!(
            result,
            Err(RunCensusError::WorkBudgetExceeded { budget: 1 })
        );
    }

    /// Independent check: brute force over all pairs of equal-length windows
    /// must agree that every selected class's copies share the exact key.
    #[test]
    fn selected_copies_agree_with_brute_force_normalization() {
        let mut code = relocatable_copy(0);
        let at = code.len() as u16;
        code.extend(relocatable_copy(at));
        let census = census_runs(&code, &whole(&code), options(8)).unwrap();
        for class in &census.classes {
            let norm = |o: &RunOccurrence| -> Vec<u8> {
                let mut out = Vec::new();
                for inst in decode(&code) {
                    if inst.pc < o.start || inst.pc >= o.end {
                        continue;
                    }
                    let bytes = &code[inst.pc as usize..(inst.pc + inst.len) as usize];
                    if inst.opcode == PUSH2 {
                        let v = u32::from(u16::from_be_bytes([bytes[1], bytes[2]]));
                        if (o.start..o.end).contains(&v) {
                            out.extend([inst.opcode, 0xee]);
                            out.extend((v - o.start).to_be_bytes());
                            continue;
                        }
                    }
                    out.extend(bytes);
                }
                out
            };
            let first = norm(&class.occurrences[0]);
            for o in &class.occurrences[1..] {
                assert_eq!(norm(o), first);
            }
        }
    }

    #[test]
    fn constants_as_ports_group_copies_that_differ_only_in_constants() {
        let mut code = body(1);
        code.push(STOP);
        code.extend(body(2));
        let strict = census_runs(&code, &whole(&code), options(12)).unwrap();
        assert!(strict.classes.is_empty());
        let loose = census_runs(
            &code,
            &whole(&code),
            RunCensusOptions {
                constants_as_ports: true,
                ..options(12)
            },
        )
        .unwrap();
        assert_eq!(loose.policy, RUN_POLICY_CONSTANT_PORTS);
        assert_eq!(loose.classes.len(), 1, "{loose:?}");
        let class = &loose.classes[0];
        assert_eq!(class.occurrences.len(), 2);
        // Two distinct constants per copy (the filler and 0x07); only the
        // filler differs between the copies.
        assert_eq!(class.constant_ports, 2);
        assert_eq!(class.varying_constant_ports, 1);
        assert_eq!(class.occurrences[0].constant_bindings, vec!["01", "07"]);
        assert_eq!(class.occurrences[1].constant_bindings, vec!["02", "07"]);
    }

    #[test]
    fn constant_ports_keep_the_equality_pattern_between_constants() {
        // Copy a uses the same value twice, copy b two different values.
        let a = vec![PUSH1, 5, DUP1, ADD, POP, PUSH1, 5, MUL, POP, DUP1, ADD, POP];
        let b = vec![PUSH1, 5, DUP1, ADD, POP, PUSH1, 6, MUL, POP, DUP1, ADD, POP];
        let mut code = a;
        code.push(STOP);
        code.extend(b);
        let loose = census_runs(
            &code,
            &whole(&code),
            RunCensusOptions {
                constants_as_ports: true,
                ..options(12)
            },
        )
        .unwrap();
        assert!(loose.classes.is_empty(), "{loose:?}");
    }

    #[test]
    fn single_instructions_are_not_runs_and_varying_constants_can_be_capped() {
        // Three PUSH32s with different values, each followed by a different
        // opcode: the only repeat is the single PUSH32 instruction.
        let mut code = Vec::new();
        for (v, next) in [(1u8, ADD), (2, MUL), (3, POP)] {
            code.push(0x7f);
            code.extend([v; 32]);
            code.push(next);
        }
        let loose = RunCensusOptions {
            constants_as_ports: true,
            ..options(8)
        };
        assert!(
            census_runs(&code, &whole(&code), loose)
                .unwrap()
                .classes
                .is_empty()
        );
        // Copies that differ in two constants survive a cap of two, not one.
        let a = vec![PUSH1, 1, DUP1, ADD, POP, PUSH1, 7, MUL, POP, DUP1, ADD, POP];
        let b = vec![PUSH1, 2, DUP1, ADD, POP, PUSH1, 8, MUL, POP, DUP1, ADD, POP];
        let mut code = a;
        code.push(STOP);
        code.extend(b);
        let capped = |max| {
            census_runs(
                &code,
                &whole(&code),
                RunCensusOptions {
                    constants_as_ports: true,
                    max_varying_constants: Some(max),
                    ..options(12)
                },
            )
            .unwrap()
            .classes
            .len()
        };
        assert_eq!(capped(2), 1);
        assert_eq!(capped(1), 0);
    }

    /// Every window of every length: the fast per-occurrence key partitions
    /// windows exactly like the core facet address does, for both facets.
    #[test]
    fn fast_key_partitions_like_the_core_facet_address() {
        let mut code = relocatable_copy(0);
        let at = code.len() as u16;
        code.extend(relocatable_copy(at));
        code.extend(body(1));
        code.extend(body(2));
        code.extend([PUSH1, 5, DUP1, PUSH1, 5, ADD, PUSH1, 6, MUL, POP]);
        code.extend([PUSH1, 7, DUP1, PUSH1, 7, ADD, PUSH1, 8, MUL, POP]);
        for blind in [false, true] {
            let stream = build_stream(&code, &whole(&code), blind);
            let facet = run_facet(blind);
            let positions = stream.insts.iter().filter(|i| i.is_some()).count();
            for len in 2..8usize {
                let mut by_fast: HashMap<(Vec<u32>, Vec<u64>), usize> = HashMap::new();
                let mut by_core: HashMap<riff_catalog_core::Digest, usize> = HashMap::new();
                let mut pairs = Vec::new();
                for p in 0..=positions.saturating_sub(len) {
                    let mut visits = 0;
                    let fast = (
                        stream.tokens[p..p + len].to_vec(),
                        occurrence_key(&stream, p, len, &mut visits).key,
                    );
                    let core = stream_address(&code, &stream, p, len, &facet);
                    let next = by_fast.len();
                    let f = *by_fast.entry(fast).or_insert(next);
                    let next = by_core.len();
                    let c = *by_core.entry(core).or_insert(next);
                    pairs.push((f, c));
                }
                for a in &pairs {
                    for b in &pairs {
                        assert_eq!(a.0 == b.0, a.1 == b.1, "blind={blind} len={len}");
                    }
                }
            }
        }
    }

    #[test]
    fn exact_facet_keeps_constants_and_blind_facet_forgets_them() {
        let run = |code: &[u8]| -> Vec<(Instruction, Option<u32>)> {
            decode(code).into_iter().map(|i| (i, None)).collect()
        };
        let a = [PUSH1, 1, DUP1, ADD];
        let b = [PUSH1, 2, DUP1, ADD];
        let ga = run_graph(&a, &run(&a));
        let gb = run_graph(&b, &run(&b));
        assert_ne!(
            run_address(&ga, &run_facet(false)),
            run_address(&gb, &run_facet(false))
        );
        assert_eq!(
            run_address(&ga, &run_facet(true)),
            run_address(&gb, &run_facet(true))
        );
        assert_ne!(run_facet(false).facet_id(), run_facet(true).facet_id());
    }
}
