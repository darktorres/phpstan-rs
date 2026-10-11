# PHPUnit comparator matrix

`matrix.php` compares every pair of a fixed set of values with
`sebastian/comparator`, as PHPUnit's `assertEquals` does, and prints JSON.
The comparator model's test (`crates/types/src/tests/comparator.rs`)
checks the model against each file in `crates/types/src/tests/data/`, one
per comparator major (4 for PHPUnit 9, 5 for 10, 6 for 11, 7 for 12).
Install a major into a scratch directory and run the harness with its
autoloader:

    mkdir -p ~/.cache/phpunit-comparator-matrix/c7 && cd ~/.cache/phpunit-comparator-matrix/c7 && echo '{}' > composer.json
    composer require --ignore-platform-reqs 'sebastian/comparator:^7'
    cd - && php tools/phpunit-comparator-matrix/matrix.php ~/.cache/phpunit-comparator-matrix/c7/vendor/autoload.php > crates/types/src/tests/data/comparator-7.json
