<?php
            /** @param array<int|null> $x
             *  @return array<int>
             */
            function f(array $x): array {
                return array_filter($x, static fn (?int $v): bool => $v !== 0);
            }
        