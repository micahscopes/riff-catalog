/-
The lockstep comparator: `lake exe check`.

Reads `lean/golden/vectors.json` (produced by the Rust dumper
`crates/riff-catalog-core/tests/lean_vectors.rs`), reconstructs each input
graph, recomputes every digest with the Lean spec, and compares byte for byte
against the Rust-produced expectations. Also verifies the pure-Lean BLAKE3
against:
  - the official known-answer vectors baked into the spec, and
  - the `blake3_kats` section of vectors.json (the SAME `blake3` crate riffcat
    ships), so the two BLAKE3s are pinned to each other.

A tiny self-contained JSON parser is used (no external dependency). The corpus
is emitted in a regular shape, but the parser is a real JSON value parser so the
file stays human-readable and serde-compatible.

Exit code is nonzero on any mismatch, so this doubles as a CI gate.
-/

import Riffcat.Schema
import Riffcat.Policy
import Riffcat.Encode
import Riffcat.Hash
import Riffcat.Reference
import Riffcat.Hash.Blake3
import Riffcat.Hash.Blake3Test

open Riffcat

/-! ## A minimal JSON parser (objects, arrays, strings, numbers, bools, null) -/

namespace Json

inductive Value where
  | str (s : String)
  | num (n : Int)
  | bool (b : Bool)
  | null
  | arr (xs : Array Value)
  | obj (kvs : Array (String × Value))
deriving Inhabited

/-- We parse over an `Array Char` with a `Nat` index, sidestepping the
`String.Pos` API (which is version-sensitive). -/
structure Parser where
  s : Array Char
  pos : Nat
deriving Inhabited

namespace Parser

def peek? (p : Parser) : Option Char :=
  if p.pos < p.s.size then some p.s[p.pos]! else none

def advance (p : Parser) : Parser := { p with pos := p.pos + 1 }

partial def skipWs (p : Parser) : Parser :=
  match p.peek? with
  | some c => if c == ' ' || c == '\n' || c == '\t' || c == '\r' then (p.advance).skipWs else p
  | none => p

/-- Parse a JSON string body (assumes the opening quote already consumed). -/
partial def parseStringBody (p : Parser) (acc : String) : Except String (String × Parser) :=
  match p.peek? with
  | none => .error "unterminated string"
  | some '"' => .ok (acc, p.advance)
  | some '\\' =>
    let p := p.advance
    match p.peek? with
    | none => .error "bad escape"
    | some c =>
      let p := p.advance
      match c with
      | '"' => parseStringBody p (acc.push '"')
      | '\\' => parseStringBody p (acc.push '\\')
      | '/' => parseStringBody p (acc.push '/')
      | 'n' => parseStringBody p (acc.push '\n')
      | 'r' => parseStringBody p (acc.push '\r')
      | 't' => parseStringBody p (acc.push '\t')
      | 'u' =>
        -- read 4 hex digits
        let hex := p
        let h0 := hex.peek?.getD '0'
        let hex := hex.advance
        let h1 := hex.peek?.getD '0'
        let hex := hex.advance
        let h2 := hex.peek?.getD '0'
        let hex := hex.advance
        let h3 := hex.peek?.getD '0'
        let hex := hex.advance
        let val := (hexv h0) * 4096 + (hexv h1) * 256 + (hexv h2) * 16 + (hexv h3)
        parseStringBody hex (acc.push (Char.ofNat val))
      | _ => parseStringBody p (acc.push c)
  | some c => parseStringBody p.advance (acc.push c)
where
  hexv (c : Char) : Nat :=
    let n := c.toNat
    if n >= 48 && n <= 57 then n - 48
    else if n >= 97 && n <= 102 then n - 97 + 10
    else if n >= 65 && n <= 70 then n - 65 + 10
    else 0

/-- Parse an integer (the corpus only ever emits integers as numbers). -/
partial def parseNumber (p : Parser) : Except String (Int × Parser) := do
  let mut p := p
  let mut neg := false
  if p.peek? == some '-' then
    neg := true
    p := p.advance
  let mut digits := ""
  let mut go := true
  while go do
    match p.peek? with
    | some c =>
      if c.isDigit then
        digits := digits.push c
        p := p.advance
      else
        go := false
    | none => go := false
  if digits.isEmpty then .error "expected number"
  else
    let n : Int := Int.ofNat digits.toNat!
    .ok (if neg then -n else n, p)

/-- Advance `n` characters. -/
def advanceN (p : Parser) (n : Nat) : Parser :=
  match n with | 0 => p | n + 1 => advanceN p.advance n

mutual

partial def parseValue (p0 : Parser) : Except String (Value × Parser) :=
  let p := p0.skipWs
  match p.peek? with
  | none => .error "unexpected end"
  | some '"' =>
    match parseStringBody p.advance "" with
    | .error e => .error e
    | .ok (s, p') => .ok (.str s, p')
  | some '{' => parseMembers p.advance #[]
  | some '[' => parseElements p.advance #[]
  | some 't' => .ok (.bool true, advanceN p 4)
  | some 'f' => .ok (.bool false, advanceN p 5)
  | some 'n' => .ok (.null, advanceN p 4)
  | some c =>
    if c == '-' || c.isDigit then
      match parseNumber p with
      | .error e => .error e
      | .ok (n, p') => .ok (.num n, p')
    else .error s!"unexpected char {c}"

/-- Parse array elements (opening `[` already consumed). -/
partial def parseElements (p0 : Parser) (acc : Array Value) : Except String (Value × Parser) :=
  let p1 := p0.skipWs
  if p1.peek? == some ']' then .ok (.arr acc, p1.advance)
  else
    match parseValue p1 with
    | .error e => .error e
    | .ok (v, p2) =>
      let p3 := p2.skipWs
      match p3.peek? with
      | some ',' => parseElements p3.advance (acc.push v)
      | some ']' => .ok (.arr (acc.push v), p3.advance)
      | _ => .error "expected , or ] in array"

/-- Parse object members (opening `{` already consumed). -/
partial def parseMembers (p0 : Parser) (acc : Array (String × Value))
    : Except String (Value × Parser) :=
  let p1 := p0.skipWs
  if p1.peek? == some '}' then .ok (.obj acc, p1.advance)
  else if p1.peek? != some '"' then .error "expected key string"
  else
    match parseStringBody p1.advance "" with
    | .error e => .error e
    | .ok (key, p2) =>
      let p3 := p2.skipWs
      if p3.peek? != some ':' then .error "expected :"
      else
        match parseValue p3.advance with
        | .error e => .error e
        | .ok (val, p4) =>
          let p5 := p4.skipWs
          match p5.peek? with
          | some ',' => parseMembers p5.advance (acc.push (key, val))
          | some '}' => .ok (.obj (acc.push (key, val)), p5.advance)
          | _ => .error "expected , or } in object"

end

end Parser

def parse (s : String) : Except String Value := do
  let (v, _) ← Parser.parseValue { s := s.toList.toArray, pos := 0 }
  .ok v

/-! ## Accessors -/

def Value.getObj? (v : Value) (key : String) : Option Value :=
  match v with
  | .obj kvs => (kvs.find? (fun (k, _) => k == key)).map (·.2)
  | _ => none

def Value.asStr! : Value → String
  | .str s => s
  | _ => ""

def Value.asInt! : Value → Int
  | .num n => n
  | _ => 0

def Value.asArr! : Value → Array Value
  | .arr xs => xs
  | _ => #[]

def Value.field (v : Value) (key : String) : Value :=
  (v.getObj? key).getD .null

end Json

/-! ## Reconstructing graphs from the op list -/

open Json

/-- Build an entity NodeKey (kind is uniformly "n" for ops other than node,
which carry their own kind). -/
private def mkEntity (kind owner loc : String) : NodeKey :=
  match EntityKey.new kind owner loc with
  | .ok ek => .entity ek
  | .error _ => .entity ⟨⟨"x"⟩, ⟨"x"⟩, ⟨"x"⟩⟩

private def dimOfStr (s : String) : Dimension :=
  (Dimension.parse s).getD .structure_

private def valueOfOp (vkind value : String) : Riffcat.Value :=
  match vkind with
  | "u64" => .u64 (UInt64.ofNat value.toNat!)
  | "i64" => .i64 (parseSignedInt value)
  | "text" => .text value
  | "bool" => .bool (value == "1")
  | "bytes" => .bytes (hexToBytes value)
  | _ => .text value
where
  parseSignedInt (s : String) : Int :=
    if s.startsWith "-" then -(Int.ofNat (s.drop 1).toNat!) else Int.ofNat s.toNat!
  hexToBytes (s : String) : ByteArray :=
    Id.run do
      let chars := s.toList.toArray
      let mut out := ByteArray.empty
      let mut i := 0
      while i + 1 < chars.size + 1 && i + 1 <= chars.size do
        if i + 1 < chars.size then
          let hi := hexv chars[i]!
          let lo := hexv chars[i+1]!
          out := out.push (UInt8.ofNat (hi * 16 + lo))
          i := i + 2
        else
          i := i + 2
      return out
  hexv (c : Char) : Nat :=
    let n := c.toNat
    if n >= 48 && n <= 57 then n - 48
    else if n >= 97 && n <= 102 then n - 97 + 10
    else 0

/-- Replay the op list into a `Graph` with the given graph key. -/
def buildGraphFromOps (gk : GraphKey) (ops : Array Json.Value) : Graph := Id.run do
  let mut nodes : List Node := []
  let mut children : List ChildEdge := []
  let mut edges : List Edge := []
  for op in ops do
    let opName := (op.field "op").asStr!
    match opName with
    | "node" =>
      let key := mkEntity (op.field "kind").asStr! (op.field "owner").asStr! (op.field "local").asStr!
      let kind : Name := ⟨(op.field "node_kind").asStr!⟩
      nodes := nodes ++ [{ key := key, kind := kind, fields := [] }]
    | "field" =>
      let owner := (op.field "owner").asStr!
      let loc := (op.field "local").asStr!
      let dim := dimOfStr (op.field "dim").asStr!
      let fname : Name := ⟨(op.field "name").asStr!⟩
      let val := valueOfOp (op.field "vkind").asStr! (op.field "value").asStr!
      let key := mkEntity "n" owner loc
      -- append the field to the matching node
      nodes := nodes.map (fun nd =>
        if nd.key.canonicalKey == key.canonicalKey then
          { nd with fields := nd.fields ++ [{ dimension := dim, name := fname, value := val }] }
        else nd)
    | "child" =>
      let p := mkEntity "n" (op.field "p_owner").asStr! (op.field "p_local").asStr!
      let c := mkEntity "n" (op.field "c_owner").asStr! (op.field "c_local").asStr!
      let label : Name := ⟨(op.field "label").asStr!⟩
      let ordinal := UInt32.ofNat (op.field "ordinal").asInt!.toNat
      children := children ++ [{ parent := p, label := label, ordinal := ordinal, child := c }]
    | "edge" =>
      let s := mkEntity "n" (op.field "s_owner").asStr! (op.field "s_local").asStr!
      let t := mkEntity "n" (op.field "t_owner").asStr! (op.field "t_local").asStr!
      let label : Name := ⟨(op.field "label").asStr!⟩
      let role := (EdgeRole.parse (op.field "role").asStr!).getD .reference
      edges := edges ++ [{ source := s, label := label, target := t, role := role }]
    | _ => pure ()
  return { graphKey := gk, nodes := nodes, children := children, edges := edges }

/-! ## The comparator -/

structure Counters where
  pass : Nat := 0
  fail : Nat := 0

def policyOfLabel (label : String) : Except String HashPolicy :=
  match label with
  | "identity_reject" => HashPolicy.new "test/1" .identityBound .reject
  | "anon_condense" => HashPolicy.new "test/1" .anonymousShape .condenseScc
  | "identity_condense" => HashPolicy.new "test/1" .identityBound .condenseScc
  | _ => .error s!"unknown policy label {label}"

/-- Build a facet by name under a policy. -/
def facetOfName (name : String) (policyId : Digest) : Facet :=
  match name with
  | "full" => Facet.full policyId
  | "names_blind" => Facet.namesBlind policyId
  | "structure_only" => Facet.structureOnly policyId
  | _ => Facet.full policyId

partial def main : IO UInt32 := do
  let path := "golden/vectors.json"
  let text ← IO.FS.readFile path
  let json ← match Json.parse text with
    | .ok v => pure v
    | .error e => do IO.eprintln s!"JSON parse error: {e}"; return 1
  let mut c : Counters := {}

  -- 1. BLAKE3 self-KATs
  let katFails := Riffcat.Blake3.runKats
  if katFails.isEmpty then
    IO.println "BLAKE3 self-KATs: PASS (all baked vectors)"
    c := { c with pass := c.pass + 1 }
  else
    IO.println s!"BLAKE3 self-KATs: FAIL ({katFails.length} mismatches)"
    c := { c with fail := c.fail + 1 }

  -- 2. BLAKE3 cross-check against the Rust blake3 crate (vectors.json)
  for kat in (json.field "blake3_kats").asArr! do
    let n := (kat.field "len").asInt!.toNat
    let expected := (kat.field "hash").asStr!
    let input := Id.run do
      let mut b := ByteArray.empty
      for i in [0:n] do b := b.push (UInt8.ofNat (i % 251))
      return b
    let got := Riffcat.Blake3.hashHex input
    if got == expected then
      c := { c with pass := c.pass + 1 }
    else
      IO.println s!"BLAKE3 crate KAT len={n}: FAIL expected {expected} got {got}"
      c := { c with fail := c.fail + 1 }

  -- 3. policy ids
  for pol in (json.field "policies").asArr! do
    let label := (pol.field "label").asStr!
    let expected := (pol.field "policy_id").asStr!
    match policyOfLabel label with
    | .error e => do IO.println s!"policy {label}: ERROR {e}"; c := { c with fail := c.fail + 1 }
    | .ok p =>
      let got := p.policyId.toHex
      if got == expected then c := { c with pass := c.pass + 1 }
      else
        IO.println s!"policy_id {label}: FAIL expected {expected} got {got}"
        c := { c with fail := c.fail + 1 }

  -- 4. vectors: graph digests + facet ids/addresses
  for vec in (json.field "vectors").asArr! do
    let name := (vec.field "name").asStr!
    let gkObj := vec.field "graph_key"
    let gk : GraphKey :=
      let owner := mkEntity (gkObj.field "kind").asStr! (gkObj.field "owner").asStr! (gkObj.field "local").asStr!
      match owner with
      | .entity ek => { owner := ek, localName := ⟨(gkObj.field "gk_local").asStr!⟩ }
      | _ => { owner := ⟨⟨"x"⟩, ⟨"x"⟩, ⟨"x"⟩⟩, localName := ⟨"x"⟩ }
    let g := buildGraphFromOps gk (vec.field "ops").asArr!
    for res in (vec.field "results").asArr! do
      let label := (res.field "policy").asStr!
      match policyOfLabel label with
      | .error _ => c := { c with fail := c.fail + 1 }
      | .ok p =>
        match Hash.digestGraph p gk g Dimension.all with
        | .error e => do
          IO.println s!"vector {name}/{label}: ERROR {e}"
          c := { c with fail := c.fail + 1 }
        | .ok gh =>
          -- graph digests per dimension
          for de in (res.field "graph").asArr! do
            let dim := dimOfStr (de.field "dim").asStr!
            let expected := (de.field "digest").asStr!
            let got := (gh.graph.get dim).map (·.toHex) |>.getD "<none>"
            if got == expected then c := { c with pass := c.pass + 1 }
            else
              IO.println s!"vector {name}/{label} dim={dim.asStr}: FAIL expected {expected} got {got}"
              c := { c with fail := c.fail + 1 }
          -- facet ids + address digests
          for fe in (res.field "facets").asArr! do
            let fname := (fe.field "name").asStr!
            let facet := facetOfName fname p.policyId
            let expectedFid := (fe.field "facet_id").asStr!
            let gotFid := facet.facetId.toHex
            if gotFid == expectedFid then c := { c with pass := c.pass + 1 }
            else
              IO.println s!"vector {name}/{label} facet {fname} id: FAIL expected {expectedFid} got {gotFid}"
              c := { c with fail := c.fail + 1 }
            let expectedAddr := (fe.field "address_digest").asStr!
            match gh.facetAddress facet with
            | .error e => do
              IO.println s!"vector {name}/{label} facet {fname} address: ERROR {e}"
              c := { c with fail := c.fail + 1 }
            | .ok addr =>
              let gotAddr := addr.addressDigest.toHex
              if gotAddr == expectedAddr then c := { c with pass := c.pass + 1 }
              else
                IO.println s!"vector {name}/{label} facet {fname} address: FAIL expected {expectedAddr} got {gotAddr}"
                c := { c with fail := c.fail + 1 }

  IO.println ""
  IO.println s!"lockstep: {c.pass} checks PASS, {c.fail} FAIL"
  if c.fail == 0 then
    IO.println "ALL GREEN: Lean spec matches the Rust corpus byte for byte."
    return 0
  else
    return 1
