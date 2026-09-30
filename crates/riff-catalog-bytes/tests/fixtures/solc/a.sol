// SPDX-License-Identifier: MIT
pragma solidity ^0.8.20;
import "./b.sol";
contract A is B {
    modifier only(uint x) { require(x > 0, "no"); _; }
    function f(uint x, bytes calldata d) external only(x) returns (uint) { return g(x) + d.length + h(d); }
    function h(bytes calldata d) internal pure returns (uint) { return abi.decode(d, (uint)); }
    function h(uint a) internal pure returns (uint) { return a * 3; }
    function k(uint a) external pure returns (uint) { return h(a); }
}
