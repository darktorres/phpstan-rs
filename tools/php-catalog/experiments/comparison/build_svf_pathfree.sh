#!/bin/bash
# Builds SVF's wpa and dvf from the pinned commit at ROOT, with no build path in
# the binaries. Usage: build_svf_pathfree.sh ROOT SVF_CLONE DEPS_PREFIX
#
#   SVF_CLONE is a checkout of SVF at the lock's svf_commit (it is copied, not
#   modified). DEPS_PREFIX is the usr/ directory of the extracted Z3 packages.
#
# The patch removes the build-directory candidate from SVF's extapi.bc lookup, so
# analysis needs `-extapi=` (see svf-relocatable-extapi.patch). Prefix maps and
# CMAKE_SKIP_BUILD_RPATH remove the root from the binaries; run with
# LD_LIBRARY_PATH pointing at DEPS_PREFIX/lib/x86_64-linux-gnu for libz3.
set -euo pipefail
ROOT=$1
SVF_CLONE=$2
DEPS_PREFIX=$3
HERE=$(cd "$(dirname "$0")" && pwd)
rm -rf "$ROOT"
mkdir -p "$ROOT"
cp -a "$SVF_CLONE" "$ROOT/SVF"
# CMake writes compile_commands.json into the directory it runs from, so run it inside ROOT.
cd "$ROOT"
git -C "$ROOT/SVF" apply "$HERE/svf-relocatable-extapi.patch"
cmake -S "$ROOT/SVF" -B "$ROOT/svf-build" -G Ninja -DCMAKE_BUILD_TYPE=Release \
  -DCMAKE_C_COMPILER=clang-21 -DCMAKE_CXX_COMPILER=clang++-21 \
  -DLLVM_DIR=/usr/lib/llvm-21/lib/cmake/llvm \
  -DZ3_DIR="$DEPS_PREFIX" \
  -DZ3_LIBRARY_DIR="$DEPS_PREFIX/lib/x86_64-linux-gnu/libz3.so" \
  -DSVF_WARN_AS_ERROR=OFF \
  -DCMAKE_SKIP_BUILD_RPATH=ON \
  -DCMAKE_C_FLAGS="-ffile-prefix-map=$ROOT=." \
  -DCMAKE_CXX_FLAGS="-ffile-prefix-map=$ROOT=."
cmake --build "$ROOT/svf-build" --target wpa dvf --parallel 2
echo "built $ROOT/svf-build/bin/{wpa,dvf} and $ROOT/svf-build/lib/extapi.bc"
