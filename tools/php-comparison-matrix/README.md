# PHP comparison matrix

`matrix.php` evaluates `==`, `<`, `<=`, `>`, `>=` and `<=>` over every pair
of a fixed set of values and prints JSON. The comparison model's test
(`crates/types/src/tests/comparison.rs`) checks the model against each
file in `crates/types/src/tests/data/`. Regenerate them after changing the
value set, or for a new PHP version:

    php tools/php-comparison-matrix/matrix.php > crates/types/src/tests/data/comparison-8.5.json
    docker run --rm -v "$PWD/tools/php-comparison-matrix:/m:ro" php:8.0-cli php /m/matrix.php > crates/types/src/tests/data/comparison-8.0.json
    docker run --rm -v "$PWD/tools/php-comparison-matrix:/m:ro" php:7.4-cli php /m/matrix.php > crates/types/src/tests/data/comparison-7.4.json
