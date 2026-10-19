# PHPUnit comparator matrix

`matrix.php` compares every pair of a fixed set of values with
`sebastian/comparator`, as PHPUnit's `assertEquals` does, and prints JSON.
The comparator model's test (`crates/types/src/tests/comparator.rs`)
checks the model against each file in `crates/types/src/tests/data/`, one
per comparator major (4 for PHPUnit 9, 5 for 10, 6 for 11, 7 for 12).

`comparator-<major>/` holds the `composer.json` and `composer.lock` of one
major, pinned to the version its data file records, so a regeneration gives
the same file. Install one outside the tree and run the harness with its
autoloader:

    COMPOSER_VENDOR_DIR="$PWD/target/comparator-7" composer install --no-interaction --working-dir=tools/phpunit-comparator-matrix/comparator-7
    php tools/phpunit-comparator-matrix/matrix.php target/comparator-7/autoload.php > crates/types/src/tests/data/comparator-7.json

To move a major to a newer release, change the version in its
`composer.json`, run `composer update` there, and regenerate the data file
in the same commit.
