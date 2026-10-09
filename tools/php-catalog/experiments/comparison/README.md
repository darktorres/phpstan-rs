# Native comparison experiments

See [the decision record](../../../../PHP_SEMANTICS_CATALOG_COMPARISON.md). These
tools compare compiler representations and solver prerequisites. They do not
generate a production catalog, certify semantic closure, or complete Phase F.

The `results` directory contains measured inputs/dependency hashes, counters,
framework output, runtime traces, and counterexamples. Large source trees,
bitcode, binaries, and complete dumps stay outside the repository. Original
scratch artifacts are in `/tmp/php-catalog-comparison`.

## Inputs and builds

Measured prerequisites: Clang/LLVM C++ development SDK 21.1.8, GCC 15.2.0,
Python 3, GNU time, Git, CMake, Ninja, autoconf, bison 3.8.2, re2c 4.4, and
Z3 development files 4.13.3. See `results/lock.json` for exact identities.
Experimental Z3 is an SVF dependency, not the future production SMT lock.

Bison, re2c, and Z3 packages were extracted locally without system installation:

```bash
mkdir -p /tmp/php-catalog-comparison
cd /tmp/php-catalog-comparison
apt-get download bison re2c libz3-dev libz3-4
for package in ./*.deb; do dpkg-deb -x "$package" deps; done
```

For an exact rerun, acquire the versions/digests in the lock. LLVM/Clang's
installed C++ SDK is required; a compiler binary alone is insufficient.
Run source/build commands from scratch: the evaluated SVF CMake creates a
compilation-database link relative to its working directory.

```bash
cd /tmp/php-catalog-comparison
git clone --depth 1 --branch php-8.5.4 https://github.com/php/php-src.git php-src
git -C php-src rev-parse HEAD
# Expected: e5e09d06c13e2d280a4c5066ac3d62d88dbf5b03
git clone https://github.com/SVF-tools/SVF.git SVF
git -C SVF checkout --detach e889a5139dbca132142b07d90192efab9a16c614
export PATH=/tmp/php-catalog-comparison/deps/usr/bin:$PATH
export BISON_PKGDATADIR=/tmp/php-catalog-comparison/deps/usr/share/bison
cd php-src
./buildconf --force
CC=clang-21 CXX=clang++-21 CFLAGS='-O0 -g' ./configure \
  --disable-all --disable-cgi --disable-phpdbg --without-pear \
  --enable-cli --enable-session
make -j2
cd /tmp/php-catalog-comparison
cmake -S SVF -B svf-build -G Ninja -DCMAKE_BUILD_TYPE=Release \
  -DCMAKE_C_COMPILER=clang-21 -DCMAKE_CXX_COMPILER=clang++-21 \
  -DLLVM_DIR=/usr/lib/llvm-21/lib/cmake/llvm \
  -DZ3_DIR=/tmp/php-catalog-comparison/deps/usr \
  -DZ3_LIBRARY_DIR=/tmp/php-catalog-comparison/deps/usr/lib/x86_64-linux-gnu/libz3.so \
  -DSVF_WARN_AS_ERROR=OFF
cmake --build svf-build --target wpa dvf --parallel 2
```

Build CALL in a separate source worktree with an explicit capability setting:

```bash
git -C /tmp/php-catalog-comparison/php-src worktree add --detach \
  /tmp/php-catalog-comparison/php-src-call e5e09d06c13e2d280a4c5066ac3d62d88dbf5b03
cd /tmp/php-catalog-comparison/php-src-call
./buildconf --force
CC=clang-21 CXX=clang++-21 CFLAGS='-O0 -g' php_cv_preserve_none=no \
  ./configure --disable-all --disable-cgi --disable-phpdbg --without-pear \
  --enable-cli --enable-session
php Zend/zend_vm_gen.php --with-vm-kind=CALL
make -j2
```

The saved builds were supervised with `Runner` from `run.py`: per-process
address-space limits, sampled group RSS, timeouts, logs, and external timing.
For measured builds invoke these commands through that runner. This worker
does not enforce the production aggregate-memory cgroup gate.

## Probes and evidence checks

From the repository root:

```bash
clang++-21 -std=c++17 -O1 -I/usr/lib/llvm-21/include \
  tools/php-catalog/experiments/comparison/native_probe.cpp \
  /usr/lib/llvm-21/lib/libclang-cpp.so.21.1 \
  -L/usr/lib/llvm-21/lib -lLLVM-21 \
  -o /tmp/php-catalog-comparison/native-probe
python3 tools/php-catalog/experiments/comparison/run.py \
  --php /tmp/php-catalog-comparison/php-src \
  --probe /tmp/php-catalog-comparison/native-probe \
  --svf /tmp/php-catalog-comparison/svf-build/bin \
  --call-php /tmp/php-catalog-comparison/php-src-call/sapi/cli/php \
  --output /tmp/php-catalog-comparison/rerun
python3 tools/php-catalog/experiments/comparison/auxiliary.py \
  --results /tmp/php-catalog-comparison/rerun \
  --output /tmp/php-catalog-comparison/rerun-auxiliary \
  --php /tmp/php-catalog-comparison/php-src \
  --probe /tmp/php-catalog-comparison/native-probe \
  --svf /tmp/php-catalog-comparison/svf-build/bin \
  --historical /home/torres/.cache/php-catalog/8.4 --large-growth
python3 tools/php-catalog/experiments/comparison/checks.py \
  --results /tmp/php-catalog-comparison/rerun \
  --aux /tmp/php-catalog-comparison/rerun-auxiliary/aux \
  --external /tmp/php-catalog-comparison/rerun-auxiliary/external-comparison \
  --output /tmp/php-catalog-comparison/rerun-findings.json
```

Omit `--historical` if the preserved export is unavailable. `--large-growth`
adds the 64 MiB static-array case, which used about 2 GiB in stock SVF.

The checks assert observed CFG and missing-model limitations. A compiler or
framework that repairs them requires reviewing the comparison. Runtime traces
do not prove absence or complete equivalence. Probe exit zero never certifies
closed effects: framework errors, exploration cutoffs, and pointer fallback
results are recorded separately.

## `strlen` instruction inventory

With the cached locked PHP tree and linked module, regenerate the source/layout
and LLVM witness, including the fail-closed opcode-family inventory:

```bash
python3 -m unittest tools/php-catalog/experiments/comparison/test_extract_strlen_llvm_witness.py
python3 tools/php-catalog/experiments/comparison/extract_strlen_llvm_witness.py \
  /home/torres/.cache/php-catalog-evaluation/php-src-gcc/Zend/zend_builtin_functions.c \
  /home/torres/.cache/php-catalog-evaluation/whole-clang/php-cli.bc \
  --output /tmp/php-catalog-comparison/strlen-llvm-operation-inventory.json
cargo run -p phpstan-rs-semantics --example build_strlen_catalog -- \
  /home/torres/.cache/php-catalog-evaluation/php-src-gcc/Zend/zend_builtin_functions.c \
  /tmp/php-catalog-comparison/strlen-llvm-operation-inventory.json \
  tools/php-catalog/experiments/comparison/results/runtime/strlen-weak-scalars-8.5.4.json \
  > /tmp/php-catalog-comparison/php-8.5.4-strlen.json
cmp crates/php-semantics/data/php-8.5.4-strlen.json \
  /tmp/php-catalog-comparison/php-8.5.4-strlen.json
```

The inventory accounts for each instruction opcode in `zif_strlen` and fails
if the selected compiler output adds an unclassified instruction. It does not
interpret those operations; the graph's per-domain coverage remains the
authority for which behaviors can be consumed. The witness also carries the
Return demand path from both parser-result destinations through the `len`
field load to the caller's return-zval payload. The catalog builder validates
that slot chain and keeps effects and non-normal outcomes open; it does not
claim a reusable native CFG or memory solver.
