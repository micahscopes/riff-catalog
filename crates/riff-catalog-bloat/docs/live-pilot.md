# Live scalar-helper pilot

This is a contained test of the measurement loop, not a general inlining
recommendation. The existing Fe fixture computes `3*x+y` through `mix_words`.
We compare the normal retained helper with an explicitly requested, backend-legal
force-inline variant. Both use the same source identity and compiler build.

## What to inspect

1. `compile-summary.json`: exact WGSL sizes and SHA-256 digests. Capturing the
   baseline must not change its WGSL compared with observer-disabled compilation.
2. `events.jsonl`: helper eligibility, baseline and selected retention decisions,
   stage-local call graphs, inline events, and separate cumulative clone census.
3. Sealed captures and comparison: independently verified artifact bytes, static
   reachable-function unions, and declared intervention differences.
4. The pixel oracle: execute the exact saved WGSL and compare every pixel with
   the independent Rust expression `3*x+y`.

The compiler event boundary has no riff-cat dependency. The toolkit ingests it
after compilation. Hashes bind the recorded evidence and output bytes; they do
not prove semantic equivalence or identify interchangeable graph regions.

## Build in this workspace

The Fe observer is a local experimental patch, not an upstream Fe feature.
Use the isolated worktrees, leaving the original working copies untouched.
Run one heavy compiler build at a time.

```sh
export TMPDIR=/workspace/tmp
export RUSTC_WRAPPER=sccache
export SCCACHE_DIR=/workspace/.sccache
export CARGO_INCREMENTAL=0
export CARGO_BUILD_JOBS=1

cd /workspace/fe-worktrees/bloat-toolkit
CARGO_TARGET_DIR=/workspace/scratch/target-fe-bloat-toolkit \
  cargo build --locked -p fe-codegen \
  --example bloat_capture_kernel --example bloat_gpu_oracle

cd /workspace/riff-catalog-worktrees/bloat-toolkit
CARGO_TARGET_DIR=/workspace/scratch/target-riffcat-bloat \
  cargo build --locked -p riff-catalog-bloat
```

## Run a new capture

Choose a new directory for each run. Existing output files are not overwritten.
The harness uses fresh child processes for each variant and clears the relevant
policy environment variables between modes.

```sh
export BLOAT_RUN=/workspace/scratch/bloat-scalar-my-run
export BLOAT_TOOL=/workspace/scratch/target-riffcat-bloat/debug/riffcat-bloat
export BLOAT_EXAMPLES=/workspace/scratch/target-fe-bloat-toolkit/debug/examples

"$BLOAT_EXAMPLES/bloat_capture_kernel" "$BLOAT_RUN"
```

This produces observer-disabled, baseline, and force-inline WGSL, two request
capture directories, and a compile summary. The summary separates frontend
package construction from shader lowering/backend/observer wall time. These
single observations are not benchmarks of either compiler policy or observer
overhead. Shader GPU compilation is not included in those compiler timings.

## Import and replay

Supply actual source and compiler identifiers, including local patches if the
worktree is dirty. Do not call an uncommitted compiler build just its base HEAD.
The source digest is in the compile summary. Keep compiler identity identical
between the two variants: the intervention is recorded separately.

```sh
export BLOAT_SOURCE='sha256:REPLACE_FROM_COMPILE_SUMMARY'
export BLOAT_COMPILER='fe:REPLACE_WITH_EXACT_REVISION_AND_PATCH_ID'
export BLOAT_REVISION='REPLACE_WITH_EXACT_REVISION_AND_PATCH_ID'

for mode in baseline force-inline-mix-words; do
  BLOAT_EVENTS=$(rg --files "$BLOAT_RUN/$mode-capture" -g events.jsonl)
  "$BLOAT_TOOL" import-events \
    --events "$BLOAT_EVENTS" \
    --output "$BLOAT_RUN/$mode.capture.json" \
    --label "scalar-helper-$mode" \
    --source-id "$BLOAT_SOURCE" \
    --compiler-id "$BLOAT_COMPILER" \
    --producer-revision "$BLOAT_REVISION" \
    --command 'bloat_capture_kernel: scalar_helper_call_render.fe' \
    --setting target=webgpu --setting profile=dev
  "$BLOAT_TOOL" replay "$BLOAT_RUN/$mode.capture.json"
done

"$BLOAT_TOOL" compare \
  "$BLOAT_RUN/baseline.capture.json" \
  "$BLOAT_RUN/force-inline-mix-words.capture.json" \
  --verify-artifacts
```

Artifact references currently use absolute paths. Keep the original captures
immutable. If moving the pilot to another machine, retain the raw request
directories and import them into new capture files there. Relocating or
relabeling a capture changes its content address, even with identical shaders.

## Execute the saved shaders

```sh
export XDG_CACHE_HOME=/workspace/.cache
export MESA_SHADER_CACHE_DIR=/workspace/.cache/mesa
export WGPU_BACKEND=vulkan
# This VM's Vulkan loader. Other hosts should use their installed loader path.
export LD_LIBRARY_PATH=/nix/store/7krvb015vp4wq7lj6v3wadjy4q9asc8q-vulkan-loader-1.4.341.0/lib

"$BLOAT_EXAMPLES/bloat_gpu_oracle" \
  "$BLOAT_RUN/baseline.wgsl" \
  "$BLOAT_RUN/force-inline-mix-words.wgsl"
```

The oracle requires a working adapter. It fails, rather than skips, if none is
available. It checks 2,313 pixels (257 by 9), including packed-color byte carries,
and reports adapter details and digests of the exact shaders and output pixels.
On this VM the available device is software Vulkan, not physical GPU hardware.
Passing is evidence for this finite domain on that device, not general program
equivalence or a GPU performance result.

On this Nix VM, `vulkaninfo` can work while the example cannot load Vulkan:
`vulkaninfo` has a loader RUNPATH that the Rust binary lacks. The explicit
`LD_LIBRARY_PATH` above resolved that observed difference. This is a runtime
configuration requirement, not a shader validation result.

The negative control below must exit unsuccessfully with a pixel mismatch. It
is valid WGSL that returns zero, so parsing alone cannot make the test pass.

```sh
"$BLOAT_EXAMPLES/bloat_gpu_oracle" \
  /workspace/fe-worktrees/bloat-toolkit/crates/codegen/examples/fixtures/bloat_gpu_oracle_zero.wgsl
```

## Scope

Static graph totals include every instruction in each captured function's layout
blocks, not only path-feasible execution. The union counts a shared function
once and terminates on recursion. Repeated clone-census observations are not
additive inline events. Original-ID survival is not rewritten-descendant tracking.

This pilot has one authored helper and one callsite. A change in its outcome is
not evidence about large multi-entry kernels. The next useful experiment is a
measured high-growth helper from a real kernel, with its backend legality and
behavior oracle established before interpreting any apparent savings.
