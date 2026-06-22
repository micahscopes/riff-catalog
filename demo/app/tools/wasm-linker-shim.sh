#!/usr/bin/env bash
# Reconstruct the wasm linker shim this box needs to build demo/app.
#
# Why this exists: the active rustc here is source-built and ships NO rust-lld,
# so `trunk build` (wasm32) fails with "linker 'lld' not found". A separate
# rustc-bootstrap derivation in the Nix store DOES contain rust-lld, but it is a
# prebuilt dynamically-linked binary whose interpreter and libs are not on the
# default loader path. This script borrows that rust-lld and wraps it in a shim
# that invokes it under glibc's ld-linux with the right --library-path.
#
# On a normal machine you do NOT need any of this: install lld (distro package
# or `rustup component add llvm-tools`) and `trunk build` just works. See BUILD.md.
#
# Idempotent. Writes:  $HOME/.cache/riff-catalog/bin/{rust-lld.real,lld}
# Use with:  PATH="$HOME/.cache/riff-catalog/bin:$PATH" trunk build
set -euo pipefail
shopt -s nullglob

OUT="$HOME/.cache/riff-catalog/bin"
mkdir -p "$OUT"

die() { echo "wasm-linker-shim: $*" >&2; exit 1; }

# 1. rust-lld from a rustc-bootstrap store path (the active rustc lacks it).
LLD=""
for c in /nix/store/*rustc-bootstrap*/lib/rustlib/x86_64-unknown-linux-gnu/bin/rust-lld; do
  LLD="$c"; break
done
[ -n "$LLD" ] || die "no rustc-bootstrap rust-lld found in /nix/store"
BOOT="${LLD%%/lib/rustlib*}"

# 2. Loader (interpreter) is baked into the ELF; its dir provides libc/libm/libpthread.
INTERP=$(readelf -l "$LLD" | grep -oE '/nix/store/[^]]*ld-linux-x86-64\.so\.2' | head -1)
[ -n "$INTERP" ] || die "could not read interpreter from $LLD"
GLIBC_LIB=$(dirname "$INTERP")

# 3. libLLVM ships in the bootstrap's own lib dir.
llvm=("$BOOT"/lib/libLLVM*.so*)
[ ${#llvm[@]} -gt 0 ] || die "no libLLVM under $BOOT/lib"

# 4. Remaining runtime deps, picked from the store (skip -static/-dev/python variants).
pick() { # <glob> <sofile>
  local d
  for d in /nix/store/*-$1/lib; do
    case "$d" in *-static/*|*-dev/*|*python*) continue;; esac
    [ -e "$d/$2" ] && { echo "$d"; return; }
  done
  die "could not locate $2 (glob $1)"
}
GCC_LIB=$(pick 'gcc-[0-9]*-lib' libgcc_s.so.1)
ZLIB_LIB=$(pick 'zlib-[0-9]*' libz.so.1)
ZSTD_LIB=$(pick 'zstd-[0-9]*' libzstd.so.1)

LIBPATH="$GLIBC_LIB:$GCC_LIB:$ZLIB_LIB:$ZSTD_LIB:$BOOT/lib"

cp -f "$LLD" "$OUT/rust-lld.real"
chmod +x "$OUT/rust-lld.real"

# The shim defaults the LLD flavor to wasm when the caller did not set one
# (rustc invokes it as `lld` for wasm targets without passing -flavor).
cat > "$OUT/lld" <<EOF
#!/bin/sh
case " \$* " in *" -flavor "*) F= ;; *) F="-flavor wasm" ;; esac
exec "$INTERP" --library-path "$LIBPATH" "$OUT/rust-lld.real" \$F "\$@"
EOF
chmod +x "$OUT/lld"

echo "wrote $OUT/lld -> $("$OUT/lld" --version 2>/dev/null | head -1 || echo '?')"
echo 'build with:  PATH="$HOME/.cache/riff-catalog/bin:$PATH" trunk build'
