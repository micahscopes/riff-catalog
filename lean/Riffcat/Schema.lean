/-
The pure-data interchange model, mirroring `crates/riff-catalog-schema`.

Mirrors: dimension.rs, value.rs, text.rs (the Name/Digest rules that matter),
key.rs (EntityKey/NodeKey/GraphKey + canonical_key joins), graph.rs
(Field, Node, ChildEdge, Edge, EdgeRole, Graph, validate).

Validation here is the same closed set of rules as Rust (non-empty Name, no
unit separator, no duplicate node, endpoints present). We model errors with a
`String` reason; the digest engine never reaches a digest on an invalid graph,
same as Rust.

These types carry no hashing; that lives in Encode/Hash, exactly as the Rust
crate boundary splits schema from core.
-/

namespace Riffcat

/-! ## text.rs: the unit separator, Name, Digest -/

/-- Separator used when joining key parts (U+001F). Excluded from every
validated text so canonical joins are unambiguous. -/
def unitSep : Char := Char.ofNat 0x1f

/-- A validated non-empty string with no unit separator. We keep the raw string
plus a proof-free wrapper; construction goes through `Name.new`. -/
structure Name where
  raw : String
deriving DecidableEq, Repr, Inhabited

def Name.new (value : String) (field : String) : Except String Name :=
  if value.isEmpty then
    .error s!"{field} must not be empty"
  else if value.any (· == unitSep) then
    .error s!"{field} must not contain the unit separator"
  else
    .ok ⟨value⟩

def Name.asStr (n : Name) : String := n.raw

/-- A 32-byte digest, stored as a `ByteArray`. Rendered as 64 lowercase hex. -/
structure Digest where
  bytes : ByteArray
deriving Inhabited

def hexDigits : Array Char :=
  #['0','1','2','3','4','5','6','7','8','9','a','b','c','d','e','f']

def Digest.toHex (d : Digest) : String :=
  Id.run do
    let mut s := ""
    for i in [0:d.bytes.size] do
      let byte := d.bytes.get! i
      s := s.push (hexDigits[byte.toNat >>> 4]!)
      s := s.push (hexDigits[byte.toNat &&& 0xf]!)
    return s

instance : BEq Digest where
  beq a b := a.toHex == b.toHex

/-! ## dimension.rs -/

inductive Dimension where
  | structure_
  | names
  | constants
  | types
  | traceEvents
deriving DecidableEq, Repr, Inhabited

namespace Dimension

/-- The closed set, in declaration order. This order is also the `BTreeMap`
iteration / sort order Rust uses, because `Dimension`'s `Ord` derives from the
variant order. Verified against Rust via the golden vectors. -/
def all : List Dimension :=
  [.structure_, .names, .constants, .types, .traceEvents]

def asStr : Dimension → String
  | .structure_   => "structure"
  | .names        => "names"
  | .constants    => "constants"
  | .types        => "types"
  | .traceEvents  => "trace_events"

def parse (s : String) : Option Dimension :=
  all.find? (fun d => d.asStr == s)

/-- Ordinal in the closed set, matching the derived `Ord` (declaration order). -/
def ord : Dimension → Nat
  | .structure_   => 0
  | .names        => 1
  | .constants    => 2
  | .types        => 3
  | .traceEvents  => 4

def lt (a b : Dimension) : Bool := a.ord < b.ord

end Dimension

/-! ## value.rs -/

inductive Value where
  | text (s : String)
  | bool (b : Bool)
  | u64 (n : UInt64)
  | i64 (n : Int)
  | bytes (b : ByteArray)
deriving Inhabited

/-- Structural equality of values, matching Rust's derived `PartialEq`. -/
def Value.beq : Value → Value → Bool
  | .text a,  .text b  => a == b
  | .bool a,  .bool b  => a == b
  | .u64 a,   .u64 b   => a == b
  | .i64 a,   .i64 b   => a == b
  | .bytes a, .bytes b => a.toList == b.toList
  | _, _ => false

instance : BEq Value := ⟨Value.beq⟩

/-- Total order on values matching Rust's derived `Ord`: first by the variant
order (Text < Bool < U64 < I64 < Bytes, the declaration order), then within a
variant by the inner value's order. This is the ordering `local_node_digests`
relies on to sort fields by `(name, value)`. -/
def Value.variantOrd : Value → Nat
  | .text _  => 0
  | .bool _  => 1
  | .u64 _   => 2
  | .i64 _   => 3
  | .bytes _ => 4

/-- Lexicographic byte order (Rust `Vec<u8>` Ord): shorter is smaller when it is
a prefix. -/
def bytesLt : List UInt8 → List UInt8 → Bool
  | [], [] => false
  | [], _ :: _ => true
  | _ :: _, [] => false
  | x :: xs, y :: ys => if x != y then x < y else bytesLt xs ys

/-- `lt a b` is true iff `a < b` under the derived `Ord`. -/
def Value.lt (a b : Value) : Bool :=
  let va := a.variantOrd
  let vb := b.variantOrd
  if va != vb then va < vb
  else
    match a, b with
    | .text x,  .text y  => x < y
    | .bool x,  .bool y  => (x == false && y == true)
    | .u64 x,   .u64 y   => x < y
    | .i64 x,   .i64 y   => x < y
    | .bytes x, .bytes y => bytesLt x.toList y.toList
    | _, _ => false

/-! ## key.rs -/

structure EntityKey where
  kind : Name
  owner : Name
  localName : Name
deriving Inhabited

def EntityKey.new (kind owner localName : String) : Except String EntityKey := do
  let kind ← Name.new kind "entity key kind"
  let owner ← Name.new owner "entity key owner"
  let localName ← Name.new localName "entity key local"
  return ⟨kind, owner, localName⟩

def EntityKey.canonicalKey (k : EntityKey) : String :=
  let sep := unitSep.toString
  s!"{k.kind.asStr}{sep}{k.owner.asStr}{sep}{k.localName.asStr}"

inductive NodeKey where
  | entity (key : EntityKey)
  | derived (owner : EntityKey) (localName : Name)
deriving Inhabited

def NodeKey.canonicalKey : NodeKey → String
  | .entity key => s!"entity:{key.canonicalKey}"
  | .derived owner localName =>
      s!"derived:{owner.canonicalKey}{unitSep.toString}{localName.asStr}"

/-- Node keys compare by their canonical key string, matching how `build`
sorts nodes and how `BTreeMap<NodeKey, _>` is iterated for the digest. -/
instance : BEq NodeKey where
  beq a b := a.canonicalKey == b.canonicalKey

structure GraphKey where
  owner : EntityKey
  localName : Name
deriving Inhabited

def GraphKey.new (owner : EntityKey) (localName : String) : Except String GraphKey := do
  let localName ← Name.new localName "graph key local"
  return ⟨owner, localName⟩

def GraphKey.canonicalKey (k : GraphKey) : String :=
  s!"{k.owner.canonicalKey}{unitSep.toString}{k.localName.asStr}"

instance : BEq GraphKey where
  beq a b := a.canonicalKey == b.canonicalKey

/-! ## graph.rs -/

structure Field where
  dimension : Dimension
  name : Name
  value : Value
deriving Inhabited

structure Node where
  key : NodeKey
  kind : Name
  fields : List Field
deriving Inhabited

structure ChildEdge where
  parent : NodeKey
  label : Name
  ordinal : UInt32
  child : NodeKey
deriving Inhabited

inductive EdgeRole where
  | graph
  | control
  | data
  | reference
  | call
  | dependency
  | origin
deriving DecidableEq, Repr, Inhabited

namespace EdgeRole

def asStr : EdgeRole → String
  | .graph      => "graph"
  | .control    => "control"
  | .data       => "data"
  | .reference  => "reference"
  | .call       => "call"
  | .dependency => "dependency"
  | .origin     => "origin"

def parse (s : String) : Option EdgeRole :=
  [EdgeRole.graph, .control, .data, .reference, .call, .dependency, .origin].find?
    (fun r => r.asStr == s)

/-- Only `Dependency` participates in cycle checks and SCC condensation. -/
def isRecursive : EdgeRole → Bool
  | .dependency => true
  | _ => false

end EdgeRole

structure Edge where
  source : NodeKey
  label : Name
  target : NodeKey
  role : EdgeRole
deriving Inhabited

/-- The canonical interchange form of one lowered unit.

Rust stores `nodes` as a `BTreeMap<NodeKey, Node>`; we keep an explicit list and
canonicalize (sort + dedup-check) where the engine relies on it, mirroring how
`IndexedGraph::build` re-sorts by canonical key anyway. -/
structure Graph where
  graphKey : GraphKey
  nodes : List Node
  children : List ChildEdge
  edges : List Edge
deriving Inhabited

namespace Graph

/-- Does a node with this key exist? -/
def hasNode (g : Graph) (key : NodeKey) : Bool :=
  g.nodes.any (fun n => n.key == key)

/-- `validate`, mirroring graph.rs: every map key equals its node's own key
(trivially true here since we store the node directly), the node set has no
duplicate canonical keys, and every child/edge endpoint is present. -/
def validate (g : Graph) : Except String Unit := do
  -- duplicate node keys
  let keys := g.nodes.map (fun n => n.key.canonicalKey)
  if keys.length != keys.eraseDups.length then
    .error "duplicate node key"
  for c in g.children do
    unless g.hasNode c.parent do .error s!"missing node key {c.parent.canonicalKey}"
    unless g.hasNode c.child do .error s!"missing node key {c.child.canonicalKey}"
  for e in g.edges do
    unless g.hasNode e.source do .error s!"missing node key {e.source.canonicalKey}"
    unless g.hasNode e.target do .error s!"missing node key {e.target.canonicalKey}"
  return ()

end Graph

end Riffcat
