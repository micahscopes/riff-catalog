/-
The canonical byte encoding, mirroring `crates/riff-catalog-core/src/encode.rs`.

This is invariant I1, the load-bearing piece: every digest in the system is
produced through `digestRecord` / `digestMeta`, so the domain-separation
discipline (magic, schema version, full policy context, record tag) is
reproduced here byte for byte. The structural-encoding equality the lockstep
harness checks is exactly the byte sequence these pushers build before it is
handed to blake3.

Byte layout, matching Rust:
  - push_str   : u64 little-endian length, then the UTF-8 bytes.
  - push_u32/64: little-endian, fixed width.
  - push_i64   : two's-complement little-endian (Rust `i64::to_le_bytes`).
  - push_digest: raw 32 bytes, no length prefix.
  - push_value : a tag string ("text"/"bool"/"u64"/"i64"/"bytes") then payload.
  - push_sorted_records: byte-lexicographic sort of the encoded records, then a
    u32 count, then the concatenation.

`Builder` is a thin wrapper over `ByteArray` so we read like the Rust `Vec<u8>`
pushers; the comparison harness uses `Builder` contents directly.
-/

import Riffcat.Schema
import Riffcat.Policy
import Riffcat.Hash.Blake3

namespace Riffcat
namespace Encode

def magic : String := "riffcat"

/-- A growable byte buffer; the `push*` functions append to it. -/
abbrev Builder := ByteArray

@[inline] def empty : Builder := ByteArray.empty

@[inline] def pushU32 (b : Builder) (v : UInt32) : Builder :=
  Id.run do
    let mut b := b
    b := b.push (v &&& 0xff).toUInt8
    b := b.push ((v >>> 8) &&& 0xff).toUInt8
    b := b.push ((v >>> 16) &&& 0xff).toUInt8
    b := b.push ((v >>> 24) &&& 0xff).toUInt8
    return b

@[inline] def pushU64 (b : Builder) (v : UInt64) : Builder :=
  Id.run do
    let mut b := b
    let mut v := v
    for _ in [0:8] do
      b := b.push (v &&& 0xff).toUInt8
      v := v >>> 8
    return b

/-- Two's-complement little-endian of an `Int` as `i64` (Rust `to_le_bytes`).
We reduce mod 2^64 into a `UInt64` (the wrapping cast Rust does implicitly when
the value is held as `i64`) and reuse the u64 layout, which is bit-identical. -/
@[inline] def pushI64 (b : Builder) (v : Int) : Builder :=
  let m : Int := v % (2^64)
  let m := if m < 0 then m + 2^64 else m
  pushU64 b (UInt64.ofNat m.toNat)

/-- push_str: u64 little-endian byte length, then the UTF-8 bytes. -/
@[inline] def pushStr (b : Builder) (s : String) : Builder :=
  let raw := s.toUTF8
  let b := pushU64 b (UInt64.ofNat raw.size)
  b ++ raw

/-- Raw 32 bytes, no length prefix. -/
@[inline] def pushDigest (b : Builder) (d : Digest) : Builder :=
  b ++ d.bytes

@[inline] def pushNodeKey (b : Builder) (k : NodeKey) : Builder :=
  pushStr b k.canonicalKey

/-- push_value: tag string then payload, matching the Rust match arms exactly. -/
def pushValue (b : Builder) (v : Value) : Builder :=
  match v with
  | .text s =>
      let b := pushStr b "text"
      pushStr b s
  | .bool x =>
      let b := pushStr b "bool"
      b.push (if x then 1 else 0)
  | .u64 n =>
      let b := pushStr b "u64"
      pushU64 b n
  | .i64 n =>
      let b := pushStr b "i64"
      pushI64 b n
  | .bytes raw =>
      let b := pushStr b "bytes"
      let b := pushU64 b (UInt64.ofNat raw.size)
      b ++ raw

/-- Byte-lexicographic comparison of two records, matching Rust `Vec<u8>` Ord
(`sort_unstable`). Shorter is smaller when it is a prefix. -/
partial def byteLt (a b : ByteArray) : Bool :=
  let rec go (i : Nat) : Bool :=
    if i ≥ a.size then
      (i < b.size)            -- a exhausted: a < b iff b has more bytes
    else if i ≥ b.size then
      false                   -- b exhausted, a not: a > b
    else
      let x := a.get! i
      let y := b.get! i
      if x != y then x < y else go (i + 1)
  go 0

/-- push_sorted_records: sort records byte-lexicographically, push a u32 count,
then concatenate. One ordering rule for every sorted multiset (invariant I5). -/
def pushSortedRecords (b : Builder) (records : List ByteArray) : Builder :=
  Id.run do
    let sorted := records.toArray.qsort byteLt
    let mut b := pushU32 b (UInt32.ofNat sorted.size)
    for r in sorted do
      b := b ++ r
    return b

/-! ## The two record wrappers (invariant I1) -/

/-- The pre-hash byte sequence of a dimension-scoped record: header commits to
the full policy context plus dimension and record tag, then the payload.

Returned as the raw bytes so the lockstep harness can compare the *structural
encoding* (pre-hash bytes) independently of the hash. -/
def recordBytes (policy : HashPolicy) (dimension : Dimension) (recordTag : String)
    (writePayload : Builder → Builder) : Builder :=
  let b := empty
  let b := pushStr b magic
  let b := pushU32 b policy.schemaVersion
  let b := pushStr b policy.algorithm.asStr
  let b := pushStr b policy.level.asStr
  let b := pushStr b dimension.asStr
  let b := pushStr b policy.viewMode.asStr
  let b := pushStr b policy.cyclePolicy.asStr
  let b := pushStr b recordTag
  writePayload b

/-- A dimension-scoped digest record. Mirrors `digest_record`. `check_supported`
is the caller's responsibility (as in Rust, where `digest_record` calls it). -/
def digestRecord (policy : HashPolicy) (dimension : Dimension) (recordTag : String)
    (writePayload : Builder → Builder) : Except String Digest := do
  policy.checkSupported
  let bytes := recordBytes policy dimension recordTag writePayload
  return ⟨Blake3.hash bytes⟩

/-- The pre-hash byte sequence of a meta record: magic + schema version + tag +
payload. -/
def metaBytes (tag : String) (writePayload : Builder → Builder) : Builder :=
  let b := empty
  let b := pushStr b magic
  let b := pushU32 b SCHEMA_VERSION
  let b := pushStr b tag
  writePayload b

/-- A policy-independent meta record. Mirrors `digest_meta`. -/
def digestMeta (tag : String) (writePayload : Builder → Builder) : Digest :=
  ⟨Blake3.hash (metaBytes tag writePayload)⟩

/-- Hash an explicit byte buffer (mirrors `digest_bytes`). -/
def digestBytes (bytes : ByteArray) : Digest := ⟨Blake3.hash bytes⟩

end Encode

/-! ## policy_id (lives with the encoding, mirrors HashPolicy::policy_id) -/

def HashPolicy.policyId (p : HashPolicy) : Digest :=
  Encode.digestMeta "riffcat.policy" fun b =>
    let b := Encode.pushU32 b p.schemaVersion
    let b := Encode.pushStr b p.algorithm.asStr
    let b := Encode.pushStr b p.level.asStr
    let b := Encode.pushStr b p.viewMode.asStr
    Encode.pushStr b p.cyclePolicy.asStr

end Riffcat
