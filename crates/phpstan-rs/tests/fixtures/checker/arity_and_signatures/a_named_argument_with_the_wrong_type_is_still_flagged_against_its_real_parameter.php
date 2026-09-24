<?php
            function f1(int $a, string $b): void {}
            function test(): void { f1(b: "x", a: "not an int"); }
        