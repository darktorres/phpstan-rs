<?php
            /** @param array<mixed> $x
             *  @return array<bool>
             */
            function f(array $x): array {
                return array_filter($x, function (mixed $v): bool {
                    return is_bool($v);
                });
            }
        