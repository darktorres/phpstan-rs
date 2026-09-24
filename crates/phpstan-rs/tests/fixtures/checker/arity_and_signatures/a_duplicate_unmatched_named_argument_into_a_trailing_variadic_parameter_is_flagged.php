<?php
            function f27(int $a, ...$rest): void {}
            function test(): void { f27(1, x: 2, x: 3); }
        