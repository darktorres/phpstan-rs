<?php
            function f25(int $a, string $b): void {}
            function test(array $rest): void { f25(...$rest, 2); }
        