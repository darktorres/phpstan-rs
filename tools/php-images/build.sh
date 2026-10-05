#!/bin/bash
# Runs in a container of the official `php:<version>-cli` image. Builds PHP
# again from the source the image ships, its VM generated with each opcode
# handler a function, by one configure with every bundled extension and
# SAPI that builds here, recording each compile command, then leaves in
# /out:
#
# - root/: the headers and the configured source tree, each at its path in
#   the container, for the catalog's clang;
# - compile_commands.json: each file's compile command, its paths inside
#   the container;
# - compiler.txt: the compiler's version and target, which the catalog's
#   clang poses as;
# - dist.tar: the installed build, /usr/local;
# - modules.txt: the modules the installed `php` loads;
# - runtime-packages.txt: the Debian packages the build's binaries load;
# - header-packages.tsv: each system header with the Debian package that
#   installs it, which names the library a function declared there is
#   from;
# - report.txt: the version, the options passed and left out, the packages
#   missing.
set -euo pipefail

here=/usr/local/share/php-images
out=/out
records=/var/lib/compile-commands
mkdir -p "$out/root" "$records"

version=$(php -r 'echo PHP_MAJOR_VERSION, ".", PHP_MINOR_VERSION;')
report="$out/report.txt"
echo "php $PHP_VERSION" > "$report"

# Whether $version is within $1..$2, both inclusive.
version_within() {
    [ "$(printf '%s\n%s\n' "$1" "$version" | sort -V | head -1)" = "$1" ] &&
        [ "$(printf '%s\n%s\n' "$version" "$2" | sort -V | head -1)" = "$version" ]
}

# Lines of a list file, comments and blank lines dropped.
entries() {
    sed -e 's/#.*//' -e '/^[[:space:]]*$/d' "$1"
}

sh "$here/apt-sources.sh"
wanted=$PHPIZE_DEPS
missing=
for package in $(entries "$here/packages"); do
    policy=$(apt-cache policy "$package" 2>/dev/null)
    if [[ $policy =~ Candidate:\ [^\(] ]]; then
        wanted="$wanted $package"
    else
        missing="$missing $package"
    fi
done
echo "packages missing:$missing" >> "$report"
DEBIAN_FRONTEND=noninteractive apt-get install -y -qq --no-install-recommends $wanted > /dev/null

docker-php-source extract
cd /usr/src/php
# The VM with each opcode handler a function of its own: the hybrid VM,
# the default, writes some handlers only as code inside its executor.
php Zend/zend_vm_gen.php --with-vm-kind=CALL > /tmp/vm-gen.log
./configure --help > /tmp/configure-help.txt

flags=()
left_out=
for flag in $(entries "$here/configure-flags"); do
    option=${flag%%=*}
    if ! grep -Eq -- "(^|[[:space:]])$option(\[|=|[[:space:]]|$)" /tmp/configure-help.txt; then
        continue
    fi
    why=
    while IFS=$'\t' read -r from to excluded reason; do
        if [ "$excluded" = "$option" ] && version_within "$from" "$to"; then
            why=$reason
        fi
    done < <(entries "$here/excluded.tsv")
    if [ -n "$why" ]; then
        left_out="$left_out$option: $why"$'\n'
    else
        flags+=("$flag")
    fi
done
printf 'options:' >> "$report"
printf ' %s' "${flags[@]}" >> "$report"
printf '\noptions left out:\n%s' "$left_out" >> "$report"

multiarch=$(dpkg-architecture --query DEB_BUILD_MULTIARCH)
ln -sf cc-record "$here/c++-record"
# The official image's compiler and linker flags.
export CFLAGS="${PHP_CFLAGS:-}" CPPFLAGS="${PHP_CPPFLAGS:-}" LDFLAGS="${PHP_LDFLAGS:-}"
CC="$here/cc-record" CXX="$here/c++-record" ./configure \
    --build="$(dpkg-architecture --query DEB_BUILD_GNU_TYPE)" \
    --with-libdir="lib/$multiarch" \
    --with-config-file-path=/usr/local/etc/php \
    --with-config-file-scan-dir=/usr/local/etc/php/conf.d \
    --enable-option-checking=fatal \
    --without-pear \
    "${flags[@]}" > /tmp/configure.log 2>&1 || {
    tail -40 /tmp/configure.log
    exit 1
}
nice -n 19 make -j"$(nproc)" > /tmp/make.log 2>&1 || {
    grep -B5 -A5 -m5 'error' /tmp/make.log || tail -40 /tmp/make.log
    exit 1
}
# The official build's shared extensions and their ini files go: this
# build compiles them in.
rm -rf "$(php-config --extension-dir)"
rm -f /usr/local/etc/php/conf.d/docker-php-ext-*.ini
# Apache's module is built for its sources only: installing it would
# enable it in an Apache configuration the image does not have.
make INSTALL_IT=: install > /tmp/make-install.log 2>&1 || {
    tail -40 /tmp/make-install.log
    exit 1
}
find /usr/local -type f -perm '/0111' -exec sh -c 'strip --strip-all "$@" 2> /dev/null || :' -- '{}' +
php -m > "$out/modules.txt"
printf '%s\n' "$(gcc -dumpfullversion)" "$(gcc -dumpmachine)" > "$out/compiler.txt"

php "$here/compile-commands.php" "$records" > "$out/compile_commands.json"

find /usr/src/php \( -name '*.o' -o -name '*.lo' -o -name '*.a' -o -name '*.la' \
    -o -name '*.so' -o -name '.libs' \) -prune -exec rm -rf {} +
php "$here/compile-commands.php" --include-dirs "$records" > /tmp/include-dirs.txt
printf '%s\n' /usr/include /usr/local/include /usr/src/php >> /tmp/include-dirs.txt
sort -u /tmp/include-dirs.txt | while read -r dir; do
    case "$dir" in
        /usr/include/* | /usr/local/include/* | /usr/src/php/*) continue ;;
    esac
    if [ -d "$dir" ]; then
        cp -rL --parents "$dir" "$out/root/"
    fi
done

tar -C / --exclude=usr/local/share/php-images -cf "$out/dist.tar" usr/local

find /usr/local -type f \( -perm -u+x -o -name '*.so' \) -print0 |
    xargs -0 -r file -N | grep -F ': ELF' | cut -d: -f1 |
    xargs -r ldd 2>/dev/null | awk '$2 == "=>" && $3 ~ /^\// { print $3 }' | sort -u |
    while read -r lib; do
        real=$(readlink -f "$lib")
        dpkg -S "$real" 2>/dev/null || dpkg -S "${real#/usr}" 2>/dev/null || true
    done | cut -d: -f1 | sort -u > "$out/runtime-packages.txt"

awk 'FNR == 1 {
        package = FILENAME
        sub(/.*\//, "", package)
        sub(/\.list$/, "", package)
        sub(/:.*/, "", package)
    }
    /^\/usr\/include\// { print $0 "\t" package }' /var/lib/dpkg/info/*.list |
    while IFS=$'\t' read -r path package; do
        if [ -f "$path" ]; then
            printf '%s\t%s\n' "$path" "$package"
        fi
    done | sort -u > "$out/header-packages.tsv"
