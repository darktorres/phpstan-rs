# PHP comparison matrix

`matrix.php` evaluates `==`, `<`, `<=`, `>`, `>=` and `<=>` over every pair
of a fixed set of values and prints JSON. The comparison model's test
(`crates/types/src/tests/comparison.rs`) checks the model against each
file in `crates/types/src/tests/data/`. Regenerate them after changing the
value set, or for a new PHP version. Each version runs in the image that
`crates/phpstan-rs/tests/runtime_oracle/php-images.lock` pins by digest, so
a regeneration runs the same PHP:

    image() { grep "^php:$1-cli@" crates/phpstan-rs/tests/runtime_oracle/php-images.lock; }
    for v in 8.5 8.0 7.4; do
      docker run --rm -v "$PWD/tools/php-comparison-matrix:/m:ro" "$(image $v)" php /m/matrix.php > crates/types/src/tests/data/comparison-$v.json
    done
