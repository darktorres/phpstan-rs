# Locked PHP 8.5.4 sources

These are the exact bytes of five PHP source files at upstream commit
`e5e09d06c13e2d280a4c5066ac3d62d88dbf5b03` (`php/php-src`, "Update versions for
PHP 8.5.4"). The strlen abrupt-outcome witness records each file's SHA-256, and
`build_strlen_catalog`'s tests check the witness against these files. Keep them
byte-identical to upstream; a changed file fails those digest checks.

They are included so the check runs in every checkout, not only where a PHP
build cache exists. `LICENSE` is PHP's own license (PHP License 3.01), which
permits redistribution with its notice.
