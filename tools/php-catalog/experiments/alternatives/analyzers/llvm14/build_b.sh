#!/bin/bash
# Runs inside seahorn/seahorn-llvm14: builds the same minimal CLI with clang-14 through wllvm and
# extracts its whole-program bitcode.
set -euo pipefail
(command -v file >/dev/null) || { apt-get update -qq >/dev/null && DEBIAN_FRONTEND=noninteractive apt-get install -y -qq file >/dev/null; }
export HOME=/work/home
pip3 install --user --quiet wllvm
export PATH=$HOME/.local/bin:$PATH
export LLVM_COMPILER=clang LLVM_CC_NAME=clang-14 LLVM_CXX_NAME=clang++-14 LLVM_LINK_NAME=llvm-link-14 LLVM_AR_NAME=llvm-ar-14
cd /work/php-src
php_cv_have__asm_goto=no ax_cv_have_func_attribute_ifunc=no CC=wllvm CXX=wllvm++ ./configure --disable-all --disable-cgi --disable-phpdbg --without-pear --enable-cli --enable-session CFLAGS='-O0 -g -Xclang -disable-O0-optnone' > /work/configure.log 2>&1
make -j8 > /work/make.log 2>&1
extract-bc sapi/cli/php -o /work/php-cli-14b.bc
ls -la /work/php-cli-14b.bc
chown -R ${HOST_UID}:${HOST_GID} /work
