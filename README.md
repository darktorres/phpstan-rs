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
alongside it - verify before running:

```sh
sha256sum -c phpstan-rs-x86_64-unknown-linux-gnu.gz.sha256
gunzip phpstan-rs-x86_64-unknown-linux-gnu.gz
chmod +x phpstan-rs-x86_64-unknown-linux-gnu
```

### Linux glibc compatibility

The two Linux binaries are built on GitHub Actions' `ubuntu-24.04`
runner image (glibc 2.39) - dynamically linking glibc means they will
not run on a distro whose own glibc is older than that (e.g. Ubuntu
20.04/22.04, Debian 11, RHEL 8/9). If GitHub later deprecates this
runner image, `.github/workflows/release.yml` is re-pinned to whatever
the current oldest-supported LTS is, and this note is updated to match
the new glibc floor at that time - it is not meant to stay accurate
indefinitely without a matching workflow change.

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

```sh
cargo build --release -p phpstan-rs
```

The default build includes the `pcre-validate` feature (a PCRE2-backed
regex-pattern-validity diagnostic); `pcre2-sys` vendors and compiles its
own C source via `cc-rs` when no system libpcre2/pkg-config is found, so
a working C toolchain for your target is required unless you build with
`--no-default-features`.

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

- **Where the time goes inside the recursion: a sampling profiler.** Build
  the `dhat` profile (release with debug info) without the hotpath
  features, and record call stacks with `perf` or `samply`; recursion folds
  into one path in the flame graph.

  ```sh
  cargo build -p phpstan-rs --profile dhat
  perf record -g --call-graph dwarf ./target/dhat/phpstan-rs check-project --config phpstan-rs.toml
  perf report
  # or: samply record ./target/dhat/phpstan-rs check-project --config phpstan-rs.toml
  ```

- **Which call sites allocate: dhat.** The `dhat-heap` feature swaps in
  dhat's allocator and writes `dhat-heap.json` on exit, for
  [dh_view](https://nnethercote.github.io/dh_view/dh_view.html).

  ```sh
  cargo build -p phpstan-rs --profile dhat --features dhat-heap
  ./target/dhat/phpstan-rs check-project --config phpstan-rs.toml
  ```

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
