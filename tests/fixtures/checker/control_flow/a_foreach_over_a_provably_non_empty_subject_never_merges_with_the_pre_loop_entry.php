<?php
            use function PHPStan\Testing\assertType;
            function test(string $order): void {
                $clauses = [];
                foreach (explode(',', $order) as $rawEntry) {
                    $clauses[] = $rawEntry;
                }
                assertType('non-empty-list<string>', $clauses);
            }
        