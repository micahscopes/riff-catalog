/-
Pure-Lean BLAKE3, default hashing mode only (no key, no derive-key, no XOF
beyond the 32-byte root output). This is the one external primitive of the
riffcat digest engine; implementing it in Lean (rather than binding a C
library) lets the lockstep harness assert true byte-for-byte equality with the
Rust `blake3` crate without any FFI or system library.

Scope and conformance:
- Implements only what riffcat uses: `blake3::hash(bytes).as_bytes()`, i.e. the
  unkeyed hash with the standard 32-byte (256-bit) root output.
- Verified against the official BLAKE3 known-answer test vectors in
  `Riffcat/Hash/Blake3Test.lean` (empty input and several standard lengths).
- Follows the BLAKE3 reference (the spec / reference Rust impl): 1024-byte
  chunks, 16 blocks of 64 bytes per chunk, the G mixing function, the 7-round
  message permutation schedule, chunk chaining values, and the left-complete
  binary tree of parent nodes with ROOT applied at the top.

All arithmetic is over 32-bit words modeled as `UInt32`; Lean's `UInt32` is
wrapping (mod 2^32), matching Rust.
-/

namespace Riffcat.Blake3

/-! ## Constants -/

/-- The eight BLAKE3 IV words (same as the SHA-256 initial hash values). -/
def IV : Array UInt32 :=
  #[0x6a09e667, 0xbb67ae85, 0x3c6ef372, 0xa54ff53a,
    0x510e527f, 0x9b05688c, 0x1f83d9ab, 0x5be0cd19]

/-- Domain-separation flags. -/
def CHUNK_START : UInt32 := 1
def CHUNK_END   : UInt32 := 2
def PARENT      : UInt32 := 4
def ROOT        : UInt32 := 8

def BLOCK_LEN : Nat := 64
def CHUNK_LEN : Nat := 1024

/-- The message-word permutation applied between rounds. -/
def MSG_PERMUTATION : Array Nat :=
  #[2, 6, 3, 10, 7, 0, 4, 13, 1, 11, 12, 5, 9, 14, 15, 8]

/-! ## Word/byte helpers -/

@[inline] def rotr (x : UInt32) (n : UInt32) : UInt32 :=
  (x >>> n) ||| (x <<< (32 - n))

/-- Read a little-endian `UInt32` from four bytes (indices `i .. i+3`). -/
@[inline] def leU32 (b : ByteArray) (i : Nat) : UInt32 :=
  (b.get! i).toUInt32
    ||| ((b.get! (i + 1)).toUInt32 <<< 8)
    ||| ((b.get! (i + 2)).toUInt32 <<< 16)
    ||| ((b.get! (i + 3)).toUInt32 <<< 24)

/-- Little-endian bytes of a `UInt32`. -/
@[inline] def u32le (x : UInt32) : Array UInt8 :=
  #[ (x &&& 0xff).toUInt8,
     ((x >>> 8) &&& 0xff).toUInt8,
     ((x >>> 16) &&& 0xff).toUInt8,
     ((x >>> 24) &&& 0xff).toUInt8 ]

/-! ## The compression function

`compress` mixes a 16-word block into the chaining value, producing the
16-word output state. For chaining values and parent nodes we keep the first 8
words; for the ROOT output we also use words 8..15 (the XOF feed-forward), but
riffcat only needs the first 32 bytes so words 0..7 suffice. -/

/-- One application of the G mixing function to a 16-word state, mixing in two
message words. Indices `a b c d` select the columns/diagonals. -/
@[inline] def g (s : Array UInt32) (a b c d : Nat) (mx my : UInt32) : Array UInt32 :=
  Id.run do
    let mut s := s
    let mut va := s[a]!
    let mut vb := s[b]!
    let mut vc := s[c]!
    let mut vd := s[d]!
    va := va + vb + mx
    vd := rotr (vd ^^^ va) 16
    vc := vc + vd
    vb := rotr (vb ^^^ vc) 12
    va := va + vb + my
    vd := rotr (vd ^^^ va) 8
    vc := vc + vd
    vb := rotr (vb ^^^ vc) 7
    s := s.set! a va
    s := s.set! b vb
    s := s.set! c vc
    s := s.set! d vd
    return s

/-- One round: four column mixes then four diagonal mixes. `m` is the current
(permuted) 16-word message schedule. -/
def round (s : Array UInt32) (m : Array UInt32) : Array UInt32 :=
  Id.run do
    let mut s := s
    -- columns
    s := g s 0 4 8 12 m[0]! m[1]!
    s := g s 1 5 9 13 m[2]! m[3]!
    s := g s 2 6 10 14 m[4]! m[5]!
    s := g s 3 7 11 15 m[6]! m[7]!
    -- diagonals
    s := g s 0 5 10 15 m[8]! m[9]!
    s := g s 1 6 11 12 m[10]! m[11]!
    s := g s 2 7 8 13 m[12]! m[13]!
    s := g s 3 4 9 14 m[14]! m[15]!
    return s

/-- Permute the message schedule by `MSG_PERMUTATION`. -/
def permute (m : Array UInt32) : Array UInt32 :=
  (Array.range 16).map (fun i => m[MSG_PERMUTATION[i]!]!)

/-- The compression function. `cv` is the 8-word chaining value, `block` the
16 message words, `counter` the 64-bit chunk counter, `blockLen` the number of
input bytes in this block, `flags` the domain flags. Returns the full 16-word
output state. -/
def compress (cv : Array UInt32) (block : Array UInt32)
    (counter : UInt64) (blockLen : UInt32) (flags : UInt32) : Array UInt32 :=
  Id.run do
    let counterLo : UInt32 := (counter &&& 0xffffffff).toUInt32
    let counterHi : UInt32 := (counter >>> 32).toUInt32
    let mut s : Array UInt32 :=
      #[ cv[0]!, cv[1]!, cv[2]!, cv[3]!,
         cv[4]!, cv[5]!, cv[6]!, cv[7]!,
         IV[0]!, IV[1]!, IV[2]!, IV[3]!,
         counterLo, counterHi, blockLen, flags ]
    let mut m := block
    -- 7 rounds, permuting the message between rounds.
    for r in [0:7] do
      s := round s m
      if r != 6 then
        m := permute m
    -- Feed-forward: first 8 words XOR the IV-half words; the upper 8 words
    -- XOR the original chaining value (only needed for XOF, kept for fidelity).
    for i in [0:8] do
      s := s.set! i (s[i]! ^^^ s[i + 8]!)
      s := s.set! (i + 8) (s[i + 8]! ^^^ cv[i]!)
    return s

/-! ## Chunk and tree processing

We process the whole input into a flat list of chunk chaining values, then fold
them into a left-complete binary tree, applying ROOT to the final compression.

riffcat inputs are tiny (well under one 1024-byte chunk in practice), but the
implementation handles arbitrary length so the KATs at 1024+ bytes pass. -/

/-- The 16 message words of `block` taken from `b` at byte offset `off`,
zero-padding the tail if fewer than 64 bytes remain. -/
def blockWords (b : ByteArray) (off : Nat) (len : Nat) : Array UInt32 :=
  Id.run do
    let mut words : Array UInt32 := Array.replicate 16 0
    for w in [0:16] do
      let base := off + w * 4
      let mut acc : UInt32 := 0
      for k in [0:4] do
        let idx := base + k
        let byte : UInt32 := if idx < off + len then (b.get! idx).toUInt32 else 0
        acc := acc ||| (byte <<< (8 * k.toUInt32))
      words := words.set! w acc
    return words

/-- Compress one chunk (`len` ≤ 1024 bytes, starting at byte `off`) at chunk
index `counter`, returning its 8-word chaining value. `isRootChunk` adds ROOT
when the whole message is a single chunk. -/
def chunkChainingValue (b : ByteArray) (off : Nat) (len : Nat)
    (counter : UInt64) (rootFlag : UInt32) : Array UInt32 :=
  Id.run do
    -- Number of 64-byte blocks (a non-empty chunk has at least one block; an
    -- empty input still compresses a single zero-length block).
    let nBlocks := if len == 0 then 1 else (len + BLOCK_LEN - 1) / BLOCK_LEN
    let mut cv := IV
    for bi in [0:nBlocks] do
      let blockOff := off + bi * BLOCK_LEN
      let blockLen : Nat :=
        if len == 0 then 0
        else min BLOCK_LEN (len - bi * BLOCK_LEN)
      let words := blockWords b blockOff blockLen
      let mut flags : UInt32 := 0
      if bi == 0 then flags := flags ||| CHUNK_START
      if bi == nBlocks - 1 then
        flags := flags ||| CHUNK_END
        flags := flags ||| rootFlag
      let out := compress cv words counter blockLen.toUInt32 flags
      cv := out.extract 0 8
    return cv

/-- Compress a parent node from two child chaining values. -/
def parentChainingValue (left right : Array UInt32) (rootFlag : UInt32) : Array UInt32 :=
  Id.run do
    let block := left ++ right
    let out := compress IV block 0 BLOCK_LEN.toUInt32 (PARENT ||| rootFlag)
    return out.extract 0 8

/-- The largest power of two strictly less than `n` (n ≥ 2). Used to split a
subtree into a left-complete part and a remainder, per the BLAKE3 tree shape. -/
partial def leftLen (n : Nat) : Nat :=
  let rec go (p : Nat) : Nat :=
    if p * 2 < n then go (p * 2) else p
  go 1

/-- Chaining value of a span of `nChunks` chunks starting at chunk index
`startChunk` (byte offset `off`, total `len` bytes). `rootFlag` is ROOT only for
the top-level call when it covers the whole input. -/
partial def cvOfChunks (b : ByteArray) (off : Nat) (len : Nat)
    (startChunk : UInt64) (nChunks : Nat) (rootFlag : UInt32) : Array UInt32 :=
  if nChunks ≤ 1 then
    chunkChainingValue b off len startChunk rootFlag
  else
    let leftChunks := leftLen nChunks
    let leftBytes := leftChunks * CHUNK_LEN
    let leftLenActual := min leftBytes len
    let left := cvOfChunks b off leftLenActual startChunk leftChunks 0
    let right := cvOfChunks b (off + leftBytes) (len - leftLenActual)
      (startChunk + leftChunks.toUInt64) (nChunks - leftChunks) 0
    parentChainingValue left right rootFlag

/-- The 32-byte BLAKE3 hash of `input` in default (unkeyed) mode. -/
def hash (input : ByteArray) : ByteArray :=
  Id.run do
    let len := input.size
    let nChunks := if len == 0 then 1 else (len + CHUNK_LEN - 1) / CHUNK_LEN
    let cv := cvOfChunks input 0 len 0 nChunks ROOT
    let mut out := ByteArray.empty
    for w in [0:8] do
      for byte in u32le cv[w]! do
        out := out.push byte
    return out

/-- Hash and render as 64 lowercase hex characters (riffcat's canonical digest
text). -/
def hexDigits : Array Char :=
  #['0','1','2','3','4','5','6','7','8','9','a','b','c','d','e','f']

def hashHex (input : ByteArray) : String :=
  Id.run do
    let bytes := hash input
    let mut s := ""
    for i in [0:bytes.size] do
      let byte := bytes.get! i
      s := s.push (hexDigits[byte.toNat >>> 4]!)
      s := s.push (hexDigits[byte.toNat &&& 0xf]!)
    return s

end Riffcat.Blake3
