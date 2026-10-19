# phpstan-rs

A Rust reimplementation of [PHPStan](https://phpstan.org/)'s static
analysis engine, plus a Language Server Protocol server (`phpstan-rs lsp`)
and a [VS Code extension](editors/vscode/) client for it.

## Installing a pre-built binary

Pre-built binaries are published on the
[Releases](../../releases) page for:

| Target | Notes |
| --- | --- |
| `x86_64-unknown-linux-gnu` | glibc-linked - see the compatibility note below |
| `aarch64-unknown-linux-gnu` | glibc-linked - see the compatibility note below |
| `aarch64-apple-darwin` | Apple Silicon |
| `x86_64-apple-darwin` | Intel Mac - Gatekeeper's quarantine attribute is not signed/notarized away; see below |
| `x86_64-pc-windows-msvc` | |

Each asset is a `.gz`-compressed binary with a `.sha256` checksum file
alongside it - verify before running. On Linux:

```sh
sha256sum -c phpstan-rs-x86_64-unknown-linux-gnu.gz.sha256
gunzip phpstan-rs-x86_64-unknown-linux-gnu.gz
chmod +x phpstan-rs-x86_64-unknown-linux-gnu
```

On macOS, which has no `sha256sum`, `shasum` reads the same checksum file:

```sh
shasum -a 256 -c phpstan-rs-aarch64-apple-darwin.gz.sha256
gunzip phpstan-rs-aarch64-apple-darwin.gz
chmod +x phpstan-rs-aarch64-apple-darwin
```

On Windows, the asset is named `phpstan-rs-x86_64-pc-windows-msvc.exe.gz`. In
PowerShell, its hash must be the checksum file's first word, and `GzipStream`
unpacks it:

```powershell
$asset = "phpstan-rs-x86_64-pc-windows-msvc.exe"
$expected = (Get-Content "$asset.gz.sha256").Split(" ")[0]
$actual = (Get-FileHash "$asset.gz" -Algorithm SHA256).Hash
if ($actual -ine $expected) { throw "checksum mismatch" }
$in = [IO.File]::OpenRead("$asset.gz")
$out = [IO.File]::Create($asset)
(New-Object IO.Compression.GzipStream($in, [IO.Compression.CompressionMode]::Decompress)).CopyTo($out)
$out.Close(); $in.Close()
```

### Linux glibc compatibility

The two Linux binaries are built on GitHub Actions' `ubuntu-24.04` and
`ubuntu-24.04-arm` runner images (glibc 2.39) - dynamically linking
glibc means they will not run on a distro whose own glibc is older than
that (e.g. Ubuntu 20.04/22.04, Debian 11, RHEL 8/9). If GitHub later
deprecates these runner images, `.github/workflows/release.yml` is
re-pinned to whatever the current oldest-supported LTS is, and this
note is updated to match the new glibc floor at that time - it is not
meant to stay accurate indefinitely without a matching workflow change.

### macOS Gatekeeper

The macOS binaries are not code-signed or notarized (this project has
no Apple Developer account). macOS's Gatekeeper will quarantine a
freshly downloaded, unsigned binary; the VS Code extension's own
downloader (`editors/vscode/src/serverDownload.ts`) strips the
quarantine attribute automatically after verifying the checksum. If you
download and run the binary yourself instead, you will need to do the
same (`xattr -d com.apple.quarantine <path>`) or approve it manually
via System Settings.

## Building from source

The build embeds the sources in the `vendor/phpstan-src` submodule, and
the repository pins a nightly toolchain (`rust-toolchain.toml`), which
`rustup` installs on the first `cargo` command in the checkout:

```sh
git submodule update --init vendor/phpstan-src
cargo build --release -p phpstan-rs
```

The default build includes the `pcre-validate` feature (a PCRE2-backed
regex-pattern-validity diagnostic); `pcre2-sys` vendors and compiles its
own C source via `cc-rs` when no system libpcre2/pkg-config is found, so
a working C toolchain for your target is required unless you build with
`--no-default-features`.

`.cargo/config.toml` builds with the compiler's parallel front end
(`-Z threads=8`) on every platform. On x86_64 Linux it links with clang
and mold; install both, or link with the system linker. `RUSTFLAGS`
replaces the flags in the configuration, so the front end's is given
again:

```sh
CARGO_TARGET_X86_64_UNKNOWN_LINUX_GNU_LINKER=cc RUSTFLAGS="-Z threads=8" \
  cargo build --release -p phpstan-rs
```

Two caches make later runs faster: the built-in stubs' registry and each
file's analysis. They are in `phpstan-rs` under the platform's cache
directory - `$XDG_CACHE_HOME` or `~/.cache` on Linux, `~/Library/Caches` on
macOS, `%LOCALAPPDATA%` on Windows - and each build keeps its own entries.
`PHPSTAN_RS_CACHE_DIR` chooses another directory, and
`PHPSTAN_RS_NO_STUB_CACHE` and `PHPSTAN_RS_NO_FILE_CACHE` turn the caches
off.

## Profiling

Three tools, each for what it measures well:

- **Phase timings and allocation totals: hotpath.** Build with
  `--features hotpath` (add `hotpath-alloc` for allocations); the report
  prints when the process exits. It times lexing, parsing, indexing, the
  registry passes and per-file checking. The recursive checker is measured
  once per outermost call (`checker::check_stmt`, `checker::check_expr`),
  since hotpath counts a function measured at every level of a recursion
  once per level.

  ```sh
  cargo build --release -p phpstan-rs --features hotpath,hotpath-alloc
  ./target/release/phpstan-rs check-project --config phpstan-rs.toml
  ```

- **Where the time goes, anywhere: a sampling profiler.** `hotpath` times
  only the functions marked for it, so it shows what was already suspected;
  a sampler needs no marks. Build the `dhat` profile (release optimizations,
  debug info, no LTO, incremental) with frame pointers, so that call stacks
  are complete, and record with `perf`. Recursion folds into one path in the
  flame graph.

  ```sh
  RUSTFLAGS="-C force-frame-pointers=yes" CARGO_TARGET_DIR=target/fp \
    cargo build -p phpstan-rs --profile dhat --bin phpstan-rs
  # run it once first: the stub cache is built on the first run after a rebuild
  perf record -F 499 -g target/fp/dhat/phpstan-rs check-project --config phpstan-rs.toml
  perf report --children --stdio -g none --no-inline --percent-limit 3
  # or: samply record target/fp/dhat/phpstan-rs check-project --config phpstan-rs.toml
  ```

  Use `--no-inline` with `perf report` and `perf script`: resolving inlined
  frames takes minutes on a debug-info binary. Frame pointers, not
  `--call-graph dwarf`, which is slow and whose stacks stop short. A run
  that scales badly with cores has serial stretches; `PHPSTAN_RS_THREADS`
  sets the worker pool's size for a scaling curve, and the call stacks of a
  window of the run show what is serial (see `CLAUDE.md`).

- **Which call sites allocate: dhat.** The `dhat-heap` feature swaps in
  dhat's allocator and writes `dhat-heap.json` on exit, for
  [dh_view](https://nnethercote.github.io/dh_view/dh_view.html).

  ```sh
  cargo build -p phpstan-rs --profile dhat --features dhat-heap
  ./target/dhat/phpstan-rs check-project --config phpstan-rs.toml
  ```

  With frame pointers kept (`RUSTFLAGS="-C force-frame-pointers=yes"`),
  `patches/dhat` walks them for each call stack, on x86_64 and aarch64, on
  Linux and macOS. On any other build, Windows included, it unwinds with the
  platform's unwinder, which is slow for whole stacks.

## VS Code extension

See [`editors/vscode/README.md`](editors/vscode/README.md).

## License

Licensed under either of

- Apache License, Version 2.0 ([`LICENSE-APACHE`](LICENSE-APACHE) or
  <https://www.apache.org/licenses/LICENSE-2.0>)
- MIT license ([`LICENSE-MIT`](LICENSE-MIT) or
  <https://opensource.org/licenses/MIT>)

at your option.

Unless you explicitly state otherwise, any contribution intentionally
submitted for inclusion in the work by you, as defined in the Apache-2.0
license, shall be dual licensed as above, without any additional terms or
conditions.
