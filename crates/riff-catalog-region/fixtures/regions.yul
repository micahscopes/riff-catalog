object "RegionPilot" {
  code {
    function straight(x) -> r {
      let a := add(x, 7)
      let b := mul(a, x)
      r := xor(b, x)
    }
    function wrapped(x) -> r {
      for { let i := 0 } lt(i, x) { i := add(i, 1) } {
        let a := add(x, 7)
        let b := mul(a, x)
        r := xor(b, x)
      }
    }
    function changed_header(x) -> r {
      for { let i := 2 } lt(i, x) { i := add(i, 3) } {
        let a := add(x, 7)
        let b := mul(a, x)
        r := xor(b, x)
      }
    }
    function kernel(parameter) -> result {
      let one := add(parameter, 7)
      let two := mul(one, parameter)
      result := xor(two, parameter)
    }
    function extracted(x) -> r {
      for { let i := 0 } lt(i, x) { i := add(i, 1) } {
        r := kernel(x)
      }
    }
    function external_a(x) -> r {
      let input := sub(x, 19)
      let a := add(input, 7)
      let b := mul(a, input)
      r := xor(b, input)
    }
    function external_b(x) -> r {
      let input := sub(x, 23)
      let a := add(input, 7)
      let b := mul(a, input)
      r := xor(b, input)
    }
    function changed_literal(x) -> r {
      let a := add(x, 8)
      let b := mul(a, x)
      r := xor(b, x)
    }
    function alias_xxy(x, y) -> r {
      let a := add(x, 7)
      let b := mul(a, x)
      r := xor(b, y)
    }
    function alias_xyy(x, y) -> r {
      let a := add(x, 7)
      let b := mul(a, y)
      r := xor(b, y)
    }
    function alias_xyx(x, y) -> r {
      let a := add(x, 7)
      let b := mul(a, y)
      r := xor(b, x)
    }
    let x := calldataload(0)
    let y := calldataload(32)
    mstore(0, straight(x))
    mstore(32, wrapped(x))
    mstore(64, changed_header(x))
    mstore(96, extracted(x))
    mstore(128, external_a(x))
    mstore(160, external_b(x))
    mstore(192, changed_literal(x))
    mstore(224, alias_xxy(x, y))
    mstore(256, alias_xyy(x, y))
    mstore(288, alias_xyx(x, y))
    return(0, 320)
  }
}
