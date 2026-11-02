# phpstan-rs

A port of [PHPStan](https://phpstan.org) to Rust.

**Status:** restarted on 2026-11-01. This branch ports PHPStan from scratch, milestone by milestone. Each milestone's finish line is a scoreboard run against PHPStan's own test suite. Nothing is usable yet; see `STATUS.md`.

- `PLAN.md`: the milestones and their exits.
- `STATUS.md`: where the work stands.
- `LESSONS.md`: what the earlier code taught.

The earlier code, which grew its own design, is kept read-only on the `legacy/*` branches and the `*-final` tags.

## Building

```sh
git clone --single-branch https://github.com/darktorres/phpstan-rs.git
cd phpstan-rs
git submodule update --init vendor/phpstan-src
cargo build --release
```

The toolchain is pinned in `rust-toolchain.toml`.

## License

MIT or Apache-2.0, at your option.
