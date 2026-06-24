/-
Known-answer tests for the pure-Lean BLAKE3, default (unkeyed) hashing mode.

The official BLAKE3 test vectors hash an input of a given length whose byte `i`
is `i mod 251`. The expected 32-byte (64 hex char) unkeyed outputs below are the
published reference values (the leading 64 hex chars of the `hash` field in the
reference `test_vectors.json`). They exercise:
  - the empty input,
  - sub-block (1 byte) and exact-block (64 byte) inputs,
  - a multi-block single chunk (1024 bytes),
  - a multi-chunk tree (1025, 2048, 2049 bytes), exercising the parent-node and
    left-complete-tree logic.

Run `#eval Riffcat.Blake3.runKats` (or `lake exe check`, which calls it) to
verify all vectors pass.
-/

import Riffcat.Hash.Blake3

namespace Riffcat.Blake3

/-- The standard test input of length `n`: byte `i` is `i mod 251`. -/
def katInput (n : Nat) : ByteArray :=
  Id.run do
    let mut b := ByteArray.empty
    for i in [0:n] do
      b := b.push (UInt8.ofNat (i % 251))
    return b

/-- (input length, expected unkeyed 32-byte hash as 64 hex chars). -/
def kats : List (Nat × String) :=
  [ (0,    "af1349b9f5f9a1a6a0404dea36dcc9499bcb25c9adc112b7cc9a93cae41f3262"),
    (1,    "2d3adedff11b61f14c886e35afa036736dcd87a74d27b5c1510225d0f592e213"),
    (64,   "4eed7141ea4a5cd4b788606bd23f46e212af9cacebacdc7d1f4c6dc7f2511b98"),
    (1023, "10108970eeda3eb932baac1428c7a2163b0e924c9a9e25b35bba72b28f70bd11"),
    (1024, "42214739f095a406f3fc83deb889744ac00df831c10daa55189b5d121c855af7"),
    (1025, "d00278ae47eb27b34faecf67b4fe263f82d5412916c1ffd97c8cb7fb814b8444"),
    (2048, "e776b6028c7cd22a4d0ba182a8bf62205d2ef576467e838ed6f2529b85fba24a"),
    (2049, "5f4d72f40d7a5f82b15ca2b2e44b1de3c2ef86c426c95c1af0b6879522563030") ]

/-- Run every KAT, returning the list of failures as
`(length, expected, got)`. Empty list means all passed. -/
def runKats : List (Nat × String × String) :=
  kats.filterMap fun (n, expected) =>
    let got := hashHex (katInput n)
    if got == expected then none else some (n, expected, got)

end Riffcat.Blake3
