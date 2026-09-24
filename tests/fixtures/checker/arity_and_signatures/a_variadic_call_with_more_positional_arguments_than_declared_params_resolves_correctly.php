<?php
            function f7(int $a, string ...$rest): void {}
            function test(): void { f7(1, "x", "y", "z"); }
        