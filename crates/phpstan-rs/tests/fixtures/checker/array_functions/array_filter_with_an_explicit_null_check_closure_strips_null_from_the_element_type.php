<?php
            /** @param array<int|null> $x
             *  @return array<int>
             */
            function f(array $x): array {
                return array_filter($x, static function (?int $v): bool {
                    return $v !== null;
                });
            }
        