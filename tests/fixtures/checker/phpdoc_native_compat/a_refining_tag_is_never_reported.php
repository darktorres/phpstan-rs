<?php
            interface Shape {}
            class Circle implements Shape {}
            /**
             * @param list<int> $ids
             * @param non-empty-string $name
             * @param Circle|null $s
             * @param int ...$rest
             * @return positive-int
             */
            function f(array $ids, string $name, ?Shape $s = null, int ...$rest): int { return 1; }
        