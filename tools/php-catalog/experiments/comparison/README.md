# Native comparison experiments

See [the decision record](../../../../PHP_SEMANTICS_CATALOG_COMPARISON.md). These
tools compare compiler representations and solver prerequisites. They do not
generate a production catalog, certify semantic closure, or complete Phase F.

The `results` directory contains measured inputs/dependency hashes, counters,
framework output, runtime traces, and counterexamples. Large source trees,
bitcode, binaries, and complete dumps stay outside the repository. Original
scratch artifacts were written to a scratch root that is no longer used; the
recorded paths are kept in `results/` as evidence. New runs use `$ROOT`,
which is set in the next section.

## Inputs and builds

Measured prerequisites: Clang/LLVM C++ development SDK 21.1.8, GCC 15.2.0,
Python 3, GNU time, Git, CMake, Ninja, autoconf, bison 3.8.2, re2c 4.4, and
Z3 development files 4.13.3. See `results/lock.json` for exact identities.
Experimental Z3 is an SVF dependency, not the future production SMT lock.

Bison, re2c, and Z3 packages were extracted locally without system installation:

```bash
export ROOT=$HOME/.cache/php-catalog-evaluation/root
mkdir -p "$ROOT"
cd "$ROOT"
apt-get download bison re2c libz3-dev libz3-4
for package in ./*.deb; do dpkg-deb -x "$package" deps; done
```

For an exact rerun, acquire the versions/digests in the lock. LLVM/Clang's
installed C++ SDK is required; a compiler binary alone is insufficient.
Run source/build commands from scratch: the evaluated SVF CMake creates a
compilation-database link relative to its working directory.

```bash
cd $ROOT
git clone --depth 1 --branch php-8.5.4 https://github.com/php/php-src.git php-src
git -C php-src rev-parse HEAD
# Expected: e5e09d06c13e2d280a4c5066ac3d62d88dbf5b03
git clone https://github.com/SVF-tools/SVF.git SVF
git -C SVF checkout --detach e889a5139dbca132142b07d90192efab9a16c614
export PATH=$ROOT/deps/usr/bin:$PATH
export BISON_PKGDATADIR=$ROOT/deps/usr/share/bison
cd php-src
./buildconf --force
CC=clang-21 CXX=clang++-21 CFLAGS='-O0 -g' ./configure \
  --disable-all --disable-cgi --disable-phpdbg --without-pear \
  --enable-cli --enable-session
make -j2
cd $ROOT
cmake -S SVF -B svf-build -G Ninja -DCMAKE_BUILD_TYPE=Release \
  -DCMAKE_C_COMPILER=clang-21 -DCMAKE_CXX_COMPILER=clang++-21 \
  -DLLVM_DIR=/usr/lib/llvm-21/lib/cmake/llvm \
  -DZ3_DIR=$ROOT/deps/usr \
  -DZ3_LIBRARY_DIR=$ROOT/deps/usr/lib/x86_64-linux-gnu/libz3.so \
  -DSVF_WARN_AS_ERROR=OFF
cmake --build svf-build --target wpa dvf --parallel 2
```

Build CALL in a separate source worktree with an explicit capability setting:

```bash
git -C $ROOT/php-src worktree add --detach \
  $ROOT/php-src-call e5e09d06c13e2d280a4c5066ac3d62d88dbf5b03
cd $ROOT/php-src-call
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
  -o $ROOT/native-probe
python3 tools/php-catalog/experiments/comparison/run.py \
  --php $ROOT/php-src \
  --probe $ROOT/native-probe \
  --svf $ROOT/svf-build/bin \
  --call-php $ROOT/php-src-call/sapi/cli/php \
  --output $ROOT/rerun
python3 tools/php-catalog/experiments/comparison/auxiliary.py \
  --results $ROOT/rerun \
  --output $ROOT/rerun-auxiliary \
  --php $ROOT/php-src \
  --probe $ROOT/native-probe \
  --svf $ROOT/svf-build/bin \
  --historical /home/torres/.cache/php-catalog/8.4 --large-growth
python3 tools/php-catalog/experiments/comparison/checks.py \
  --results $ROOT/rerun \
  --aux $ROOT/rerun-auxiliary/auxiliary \
  --external $ROOT/rerun-auxiliary/external-comparison \
  --output $ROOT/rerun-findings.json
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
  --output $ROOT/strlen-llvm-operation-inventory.json
cargo run -p phpstan-rs-semantics --example build_strlen_catalog -- \
  /home/torres/.cache/php-catalog-evaluation/php-src-gcc/Zend/zend_builtin_functions.c \
  $ROOT/strlen-llvm-operation-inventory.json \
  tools/php-catalog/experiments/comparison/results/runtime/strlen-weak-scalars-8.5.4.json \
  > $ROOT/php-8.5.4-strlen.json
cmp crates/php-semantics/data/php-8.5.4-strlen.json \
  $ROOT/php-8.5.4-strlen.json
```

The inventory accounts for each instruction opcode in `zif_strlen` and fails
if the selected compiler output adds an unclassified instruction. It does not
interpret those operations; the graph's per-domain coverage remains the
authority for which behaviors can be consumed. The witness also carries the
Return demand path from both parser-result destinations through the `len`
field load to the caller's return-zval payload. The catalog builder validates
that slot chain and keeps effects and non-normal outcomes open; it does not
claim a reusable native CFG or memory solver.

## `strlen` representation correspondence

`results/evaluation/strlen-representation-correspondence.json` is regenerated
from the checked-in evidence and the locked PHP 8.5.4 source:

```bash
python3 -m unittest tools/php-catalog/experiments/comparison/test_representation_output_binding.py
python3 tools/php-catalog/experiments/comparison/regenerate_correspondence.py \
  --source ~/.cache/php-catalog-evaluation/php-src-gcc/Zend/zend_builtin_functions.c \
  --output $ROOT/correspondence/strlen-representation-correspondence.json
cmp $ROOT/correspondence/strlen-representation-correspondence.json \
  tools/php-catalog/experiments/comparison/results/evaluation/strlen-representation-correspondence.json
```

The builder refuses any input whose digest differs from
`representation_correspondence_inputs` in `results/lock.json` before it parses
anything, and the source is identified by its digest. The recorded command
paths are mapped to their location under `recorded_root` in the lock (the root
the evidence was recorded under); `--recorded-root` names another root for
outputs recorded elsewhere. Paths are compared relative to that root, so the
same evidence checks under any location, and a path that leaves the root after
normalisation is refused.

## `strlen` SVF fallback regression

Replay the bounded indirect-call query against the linked PHP 8.5.4 module:

```bash
python3 -m unittest tools/php-catalog/experiments/comparison/test_evaluate_strlen_svf_demand.py
python3 tools/php-catalog/experiments/comparison/evaluate_strlen_svf_demand.py \
  --linked-bitcode /home/torres/.cache/php-catalog-evaluation/whole-clang/php-cli.bc \
  --svf-bin $ROOT/svf-build/bin \
  --output tools/php-catalog/experiments/comparison/results/evaluation/strlen-native-demand-svf
```

The replay extracts and verifies two temporary capsules, then runs stock DDA,
`-blk`, `-flow-bg=1`, and a capsule augmented with `std_object_handlers` and
`zend_std_cast_object_tostring`. The default, blackhole, and augmented runs
complete with an empty target at `zend_API.c:769`; the constrained run marks
the query out of budget. The pinned runtime witness has a positive
`Stringable::__toString()` callback sentinel. The capsule omits the input
object's construction/handler initialization path, so its empty target is
incomplete evidence and must leave invocation/effect behavior open. The result
JSON contains identities, command measurements, and extracted hashes; raw SVF
outputs are retained alongside it, while bitcode capsules are removed after
each replay.
