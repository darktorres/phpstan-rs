<?php
            function f24(int $a, string $b): void {}
            function test(array $rest): void { f24(a: 1, ...$rest); }
        