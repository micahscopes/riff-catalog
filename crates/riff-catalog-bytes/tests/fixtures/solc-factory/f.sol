// SPDX-License-Identifier: MIT
pragma solidity ^0.8.0;
contract Child { uint x; function set(uint v) external { x = v; } }
contract Factory {
    function make() external returns (address) { return address(new Child()); }
    function code() external pure returns (bytes memory) { return type(Child).creationCode; }
    function lit() external pure returns (string memory) { return "a long string literal that is longer than thirty-two bytes for sure yes"; }
}
library L { function f(uint a) internal pure returns (uint) { return a * 7 + 3; } }
contract Imm { uint immutable k; constructor(uint v) { k = v; } function g(uint a) external view returns (uint) { return L.f(a) + k; } }
