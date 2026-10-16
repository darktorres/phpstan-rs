#!/bin/bash
set -euo pipefail
cd /work
llvm-dis-14 php-cli-14.bc -o php-cli-14.ll
sed -E -i 's/\boptnone\b//g' php-cli-14.ll
llvm-as-14 php-cli-14.ll -o php-cli-14-noopt.bc
rm php-cli-14.ll
opt-14 -mem2reg php-cli-14-noopt.bc -o php-cli-14-m2r.bc
rm php-cli-14-noopt.bc
ls -la php-cli-14-m2r.bc
chown ${HOST_UID}:${HOST_GID} php-cli-14-m2r.bc
