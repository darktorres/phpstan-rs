<?php
            use function PHPStan\Testing\assertType;
            /**
             * @template T of array
             * @param T $a
             * @return T['id']
             */
            function id_of(array $a): mixed { return $a['id']; }
            /** @param array{id: int, name: string} $row */
            function f(array $row): void {
                assertType('int', id_of($row));
            }
        