<?php
            /** @return array{a: int, b: string} */
            function inner(): array {
                return ['a' => 1, 'b' => 'x'];
            }
            /** @return array{a: int, b: string, c: bool} */
            function outer(): array {
                return [...inner(), 'c' => true];
            }
        