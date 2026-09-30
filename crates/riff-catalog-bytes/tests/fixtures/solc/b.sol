// SPDX-License-Identifier: MIT
pragma solidity ^0.8.20;
contract B {
    uint s;
    function g(uint x) internal returns (uint) { s += x; return s * 2; }
}
