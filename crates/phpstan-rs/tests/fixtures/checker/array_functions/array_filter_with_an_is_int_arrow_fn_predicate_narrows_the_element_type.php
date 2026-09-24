<?php
            /** @param array<mixed> $x
             *  @return array<int>
             */
            function f(array $x): array {
                return array_filter($x, fn (mixed $v): bool => is_int($v));
            }
        