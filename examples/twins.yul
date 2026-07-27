// Two functions with the same shape and different names (sum_to,
// total_upto), plus one genuinely different function (scale).
// Input for 01-offline-yul.sh: ingested by riffcat's own Yul parser,
// no solc and no network involved.
object "Twins" {
  code {
    function sum_to(n) -> s {
      s := 0
      for { let i := 0 } lt(i, n) { i := add(i, 1) } {
        s := add(s, i)
      }
    }
    function total_upto(k) -> acc {
      acc := 0
      for { let j := 0 } lt(j, k) { j := add(j, 1) } {
        acc := add(acc, j)
      }
    }
    function scale(x, f) -> y {
      y := mul(x, f)
    }
    mstore(0, add(sum_to(10), total_upto(10)))
    mstore(32, scale(mload(0), 3))
    return(0, 64)
  }
}
