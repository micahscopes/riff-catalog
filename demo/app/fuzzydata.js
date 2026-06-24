// Data for the "modified variants" chapter: real deployed forks of OpenZeppelin's
// vulnerable Multicall (the ERC-2771 _msgSender-spoofing footgun) that EDITED the
// function body, so exact whole-function matching (and Sourcify's byte-identical
// match, and a name/text search) all miss them, but weighted containment over the
// engine's per-node Merkle digests still recognizes the dangerous shape.
//
// Numbers and bodies are verbatim from demo/vuln-fuzzy-vehicle-2026-06-24.md
// (the scratch `nodedump` + `sim.py` run, Sourcify-verified, a floor). The metric
// is the proposed `riffcat similar`; it is precomputed here (the engine ships the
// node digests, not yet the query).
window.RIFFCAT_FUZZY = {
  generated: "2026-06-24",
  bug: "OpenZeppelin's Multicall, mixed with ERC-2771 meta-transactions, lets an attacker spoof _msgSender: the pre-fix multicall delegate-calls each payload WITHOUT re-appending the trusted forwarder context. The v4.9.4 fix appends it.",
  nullCeiling: 0.33,
  patchedScore: 0.22,
  vuln: {
    label: "vulnerable multicall",
    sub: "OpenZeppelin pre-4.9.4",
    nodes: 49,
    fp: "c80e6c0c",
    src: `function multicall(bytes[] calldata data) external virtual returns (bytes[] memory results) {
    results = new bytes[](data.length);
    for (uint256 i = 0; i < data.length; i++) {
        results[i] = Address.functionDelegateCall(address(this), data[i]);
    }
    return results;
}`,
  },
  // Real deployed forks that edited the body. Cw = weighted containment of the
  // vulnerable shape in the fork (1.0 = exact; >0.33 = above the coincidence
  // floor; the patched shape scores ~0.22).
  variants: [
    {
      name: "LazyMintERC1155", chain: 137, chainName: "Polygon",
      address: "0xREDACTED_ADDRESS_REMOVED_FROM_HISTORY",
      cwVuln: 0.819, cwPatch: 0.218, nodes: 49,
      edit: "Identical to the vulnerable body except a dropped `virtual` modifier. That one token flips the whole-function digest, so exact matching misses it, yet the delegatecall-loop is intact.",
      src: `function multicall(bytes[] calldata data) external returns (bytes[] memory results) {
    results = new bytes[](data.length);
    for (uint256 i = 0; i < data.length; i++) {
        results[i] = Address.functionDelegateCall(address(this), data[i]);
    }
    return results;
}`,
    },
    {
      name: "LazyMintERC721", chain: 369, chainName: "PulseChain",
      address: "0xREDACTED_ADDRESS_REMOVED_FROM_HISTORY",
      cwVuln: 0.819, cwPatch: 0.218, nodes: 49,
      edit: "The same one-token (`virtual`) delta as LazyMintERC1155, on a different chain.",
      src: `function multicall(bytes[] calldata data) external returns (bytes[] memory results) {
    results = new bytes[](data.length);
    for (uint256 i = 0; i < data.length; i++) {
        results[i] = Address.functionDelegateCall(address(this), data[i]);
    }
    return results;
}`,
    },
    {
      name: "MarketplaceV3", chain: 137, chainName: "Polygon",
      address: "0xREDACTED_ADDRESS_REMOVED_FROM_HISTORY",
      cwVuln: 0.568, cwPatch: 0.257, nodes: 84,
      edit: "thirdweb's hand-rolled forwarder: added _msgSender/isForwarder locals and an if/else inside the loop using abi.encodePacked. A genuinely restructured 84-node body, distinct from BOTH the vulnerable shape and the OZ patch, so exact sorts it into neither class. The delegatecall-loop heavy subtrees survive.",
      src: `function multicall(bytes[] calldata data) external returns (bytes[] memory results) {
    results = new bytes[](data.length);
    address sender = _msgSender();
    bool isForwarder = msg.sender != sender;
    for (uint256 i = 0; i < data.length; i++) {
        if (isForwarder) {
            results[i] = Address.functionDelegateCall(address(this), abi.encodePacked(data[i], sender));
        } else {
            results[i] = Address.functionDelegateCall(address(this), data[i]);
        }
    }
    return results;
}`,
    },
    {
      name: "MarketplaceV3", chain: 8453, chainName: "Base",
      address: "0xREDACTED_ADDRESS_REMOVED_FROM_HISTORY",
      cwVuln: 0.568, cwPatch: 0.257, nodes: 84,
      edit: "Same hand-rolled forwarder restructure as the Polygon MarketplaceV3, redeployed on Base.",
      src: `function multicall(bytes[] calldata data) external returns (bytes[] memory results) {
    results = new bytes[](data.length);
    address sender = _msgSender();
    bool isForwarder = msg.sender != sender;
    for (uint256 i = 0; i < data.length; i++) {
        if (isForwarder) {
            results[i] = Address.functionDelegateCall(address(this), abi.encodePacked(data[i], sender));
        } else {
            results[i] = Address.functionDelegateCall(address(this), data[i]);
        }
    }
    return results;
}`,
    },
    {
      name: "SilMarketplaceV3", chain: 8453, chainName: "Base",
      address: "0xREDACTED_ADDRESS_REMOVED_FROM_HISTORY",
      cwVuln: 0.568, cwPatch: 0.257, nodes: 84,
      edit: "A rebranded MarketplaceV3 fork carrying the same restructured forwarder body.",
      src: `function multicall(bytes[] calldata data) external returns (bytes[] memory results) {
    results = new bytes[](data.length);
    address sender = _msgSender();
    bool isForwarder = msg.sender != sender;
    for (uint256 i = 0; i < data.length; i++) {
        if (isForwarder) {
            results[i] = Address.functionDelegateCall(address(this), abi.encodePacked(data[i], sender));
        } else {
            results[i] = Address.functionDelegateCall(address(this), data[i]);
        }
    }
    return results;
}`,
    },
  ],
  // honestly reported: real customized vulnerable bodies whose score lands near
  // the null floor, so the metric alone cannot certify them.
  marginal: [
    { name: "SwapRouter02", chainName: "Mumbai", cwVuln: 0.358, note: "Uniswap-style public payable multicall with assembly revert-decode; just above the null, not certifiable by score alone." },
    { name: "AccessControlRegistry", chainName: "Sepolia", cwVuln: 0.251, note: "unchecked loop + inline-assembly revert bubbling; sits in the null band." },
  ],
};
