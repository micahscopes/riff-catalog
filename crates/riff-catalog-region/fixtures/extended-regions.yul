object "ExtendedRegions" {
  code {
    let input_n := calldataload(0)
    let input_m := calldataload(32)
    mstore(0, nested(input_n, input_m))
    mstore(32, exits(input_n, input_m))
    effects(input_n, input_m)
    effects_swapped(input_n, input_m)
    return(0, 64)
    function nested(n, m) -> s {
      for { let i := 0 } lt(i, n) { i := add(i, 1) } {
        for { let j := 0 } lt(j, m) { j := add(j, 1) } {
          s := add(s, xor(i, j))
        }
      }
    }
    function exits(n, limit) -> s {
      for { let i := 0 } lt(i, n) { i := add(i, 1) } {
        if eq(i, limit) { break }
        s := add(s, i)
      }
    }
    function effects(a, b) {
      mstore(0, a)
      mstore(32, b)
    }
    function effects_swapped(a, b) {
      mstore(32, b)
      mstore(0, a)
    }
  }
}
