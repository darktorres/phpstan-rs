<?php
            function foo(): int { return 1; }
            function test(array $x) {
                [foo()] = $x;
            }
        