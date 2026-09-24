<?php
            function f26(int $a, ...$rest): void {}
            function test(): void { f26(1, x: 2); }
        