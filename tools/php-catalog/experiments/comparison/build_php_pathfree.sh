#!/bin/bash
# Builds the php CLI at the pinned PHP commit with no build path or build time in
# the binary. Usage: build_php_pathfree.sh ROOT PHP_CLONE DEPS_PREFIX [CALL]
#
#   With CALL, the Zend VM is generated for the CALL kind (the README's CALL build)
#   and the configure check that selects the preserve-none calling convention is off.
#
#   PHP_CLONE is a git checkout of php-src at the lock's php_commit (it is cloned,
#   not modified). DEPS_PREFIX is the usr/ directory of the extracted bison and re2c.
#
# __FILE__ is mapped to a relative path, and __DATE__/__TIME__ are fixed, so the
# binary depends on the source and the flags only. `php -v` therefore reports a
# fixed build time; the real build time is not recorded.
set -euo pipefail
HERE=$(cd "$(dirname "$0")" && pwd)
ROOT=$1
PHP_CLONE=$2
DEPS_PREFIX=$3
rm -rf "$ROOT"
mkdir -p "$ROOT"
# A clone carries only tracked files, so no object file from another build is reused.
git clone --quiet --no-hardlinks "$PHP_CLONE" "$ROOT/php-src"
export PATH="$DEPS_PREFIX/bin:$PATH"
export BISON_PKGDATADIR="$DEPS_PREFIX/share/bison"
cd "$ROOT/php-src"
./buildconf --force
if [ "${4:-}" = CALL ]; then
  export php_cv_preserve_none=no
fi
CC=clang-21 CXX=clang++-21 \
CFLAGS="-O0 -g -ffile-prefix-map=$ROOT=. -include $HERE/php-fixed-time.h" \
  ./configure --disable-all --disable-cgi --disable-phpdbg --without-pear \
  --enable-cli --enable-session
if [ "${4:-}" = CALL ]; then
  php Zend/zend_vm_gen.php --with-vm-kind=CALL
fi
# The configured srcdir is absolute, and bison writes the output path into the
# include guards of its generated headers. Rewriting the Makefile's root to `.`
# keeps the generated files root-independent; the rules still match because
# every rule uses the same spelling.
sed -i "s#$(pwd)#.#g" Makefile
make -j2
echo "built $ROOT/php-src/sapi/cli/php"
