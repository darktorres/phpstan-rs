<?php
            use function PHPStan\Testing\assertType;
            /**
             * @template T of array
             * @param T $a
             * @return key-of<T>
             */
            function first_key(array $a): int|string { return 0; }
            /** @param array{id: int, name: string} $row */
            function f(array $row): void {
                assertType("'id'|'name'", first_key($row));
            }
        