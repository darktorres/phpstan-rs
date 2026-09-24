<?php
            use function PHPStan\Testing\assertType;
            function f(string $orderByField): void {
                $orderByDate = false;
                if (str_starts_with($orderByField, 'date_')) {
                    $orderByDate = true;
                    $refDates = [1, 2, 3];
                }
                echo "unrelated code in between";
                if ($orderByDate) {
                    assertType('array{0: 1, 1: 2, 2: 3}', $refDates);
                }
            }
        