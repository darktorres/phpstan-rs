<?php
            function f28(int $a, int $b, int $extra = 0): void {}
            function test(array $args): void { f28(...$args, extra: 5); }
        