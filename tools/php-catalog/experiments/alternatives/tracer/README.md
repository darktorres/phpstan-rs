# User-code tracer

A PHP extension that records, for every internal function call, the user code that runs
directly inside it: the user function or method, the class of its object, and where that
object or closure sits among the internal call's arguments (`N`, `N[]` for an element of an
array argument, `this` for the receiver). Each process appends its counts to the file named by
`cattrace.output` when it shuts down.

It sets `zend_execute_internal`, so the compiler emits no frameless calls, and
`ZEND_COMPILE_NO_BUILTINS`, so `strlen()` and the other special functions stay real calls.

Build and run php-src's test suite through it (host PHP 8.5.4, profiler modules left out of the
ini because they hook the same entry points):

```sh
phpize && ./configure --enable-cattrace && make
php run-tests.php -p /usr/bin/php -n -c php.ini -q -j8 --set-timeout 120
python3 compare.py phpt.tsv crates/phpstan-rs/src/registry/data/builtin-runs-8.5.tsv out/
```

`php.ini` is the host's `php.ini` plus every `conf.d` file except excimer, pcov, SPX, xhprof and
opcache, plus `extension=cattrace.so` and `cattrace.output=phpt.tsv`. `results/` holds the
comparison of one run with the table.
