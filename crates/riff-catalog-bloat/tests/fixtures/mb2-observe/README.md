# Real MB2 typed-recorder compatibility fixtures

These are compiler observations, not synthetic examples. The six request files
are byte-for-byte copies from `/workspace/scratch/mb2-observe-compat-20260905`.
Complete request: `request-422142-1788624130038838429-fullscreen`.
Partial request: `request-422211-1788624170075624758-fullscreen`.
Only their containing directories changed. Historical environment paths in the
records are provenance, not local dependencies. Artifact paths are relative.

Fe HEAD: `6cc22779a2627d66f2c1bf50fd5724611ec292bc`, plus producer patch SHA-256
`7b916085cb6f70f414b1ad8c1c97c6034ecc66c68828543c6ef11123b2d5092b`.
Sonatina local overlay: `ca5210d1ff41af48d893c82f2a8380ada3e3f5c6`.
The full producer patch is deliberately not imported: it contains unrelated edits.
Source: `scalar_helper_call_render.fe`, SHA-256
`cf552b2a58e8d263d8a4ac618a49a894f172704b6e6a2cd391147658e13bd1bd`.

The partial producer used `FE_OBSERVE_MAX_EVENTS=22`. Budget exhaustion prevented
the completion marker, but compilation succeeded with an out-of-band warning.
It is not a manually truncated complete stream. Both requests contain identical
518-byte WGSL and 1300-byte SPIR-V. Their producer SHA-256 values remain in the
raw artifact records and are checked during import.

Tests cover real import, artifact-verified CLI replay, complete/partial status,
deterministic replay and same-length tampering of each artifact in both requests.
They do not rebuild the producer or repeat its reported software Vulkan oracle.
