<?php
            use function PHPStan\Testing\assertType;
            function f(): void {
                $items = [20, 50, 'all'];
                assertType('array{0: 20, 1: 50}', array_filter($items, is_int(...)));
                assertType('array{0: 20, 1: 50}', array_filter($items, 'is_int'));
                if (! in_array(20, array_filter($items, is_int(...)), true)) {
                    echo 'missing';
                }
            }
        