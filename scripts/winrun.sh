#!/usr/bin/env bash
# winrun.sh <cargo command> <cargo args...>: runs cargo for
# x86_64-pc-windows-gnu on Linux. The build links with MinGW's posix-thread
# gcc and the tests run under wine, so they meet Windows' own std::path.
# Linux only. It needs `rustup target add x86_64-pc-windows-gnu`, MinGW
# (Debian and Ubuntu: gcc-mingw-w64-x86-64) and wine64.
#
#   scripts/winrun.sh test -p phpstan-rs-path-bytes
#   scripts/winrun.sh bench -p phpstan-rs-path-bytes
#   scripts/winrun.sh test -p phpstan-rs --test fixtures
#   scripts/winrun.sh clippy --workspace --all-targets -- -D warnings
#
# The build keeps a target directory and a wine prefix of its own under
# $PRS_WIN_DIR (default ~/.cache/phpstan-rs-win), so the Linux build is not
# disturbed; CARGO_TARGET_DIR, WINEPREFIX and TMPDIR, when set, are used as
# they are. This is not the MSVC build the Windows leg runs, and wine neither
# makes a junction or a symbolic link nor lists one as Windows does: the link
# tests fail there and only there. The leg has the last word.
set -euo pipefail

if [ $# -eq 0 ]; then
  sed -n '2,/^set -e/p' "$0" | sed '$d' | cut -c3-
  exit 2
fi

win=${PRS_WIN_DIR:-$HOME/.cache/phpstan-rs-win}
mkdir -p "$win"
export CARGO_TARGET_DIR=${CARGO_TARGET_DIR:-$win/target}
export TMPDIR=${TMPDIR:-$win/tmp}
mkdir -p "$TMPDIR"

export CARGO_TARGET_X86_64_PC_WINDOWS_GNU_LINKER=x86_64-w64-mingw32-gcc-posix
export CARGO_TARGET_X86_64_PC_WINDOWS_GNU_RUNNER=wine64
export CC_x86_64_pc_windows_gnu=x86_64-w64-mingw32-gcc-posix
export CXX_x86_64_pc_windows_gnu=x86_64-w64-mingw32-g++-posix
export AR_x86_64_pc_windows_gnu=x86_64-w64-mingw32-gcc-ar

# A prefix of its own, no display, and no Mono or Gecko.
export WINEPREFIX=${WINEPREFIX:-$win/wineprefix}
export WINEARCH=win64
export WINEDEBUG=-all
export WINEDLLOVERRIDES="mscoree,mshtml="
export DISPLAY=

# The posix flavor's runtime DLLs, for a binary that wants them, by wine's
# name for a path of the host (drive Z:).
runtime=$(dirname "$(x86_64-w64-mingw32-gcc-posix -print-libgcc-file-name)")
system=/usr/x86_64-w64-mingw32/lib
export WINEPATH="Z:${runtime//\//\\};Z:${system//\//\\}"

cmd=$1
shift
exec nice -n 19 cargo "$cmd" --target x86_64-pc-windows-gnu "$@"
