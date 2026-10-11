#!/bin/sh
# Refreshes the package lists, from the Debian archive when the release the
# image is based on has left the mirrors.
set -eu

codename=$(. /etc/os-release && echo "$VERSION_CODENAME")

if apt-get update -qq 2>/var/cache/apt-update.err && ! grep -q '^[EW]:' /var/cache/apt-update.err; then
    exit 0
fi

case "$codename" in
    stretch | buster) security="$codename/updates" ;;
    *) security="$codename-security" ;;
esac
rm -f /etc/apt/sources.list.d/debian.sources
cat > /etc/apt/sources.list <<EOF
deb http://archive.debian.org/debian $codename main
deb http://archive.debian.org/debian-security $security main
EOF
echo 'Acquire::Check-Valid-Until "false";' > /etc/apt/apt.conf.d/99archive
apt-get update -qq
