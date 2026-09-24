<?php
            function f(array $x): array {
                return ['a' => 1, ...$x, 'a' => 2];
            }
        