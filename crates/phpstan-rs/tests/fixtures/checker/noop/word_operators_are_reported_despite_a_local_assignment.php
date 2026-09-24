<?php
            function test(bool $b): void {
                $x = true and $b;
                $y = false or $b;
                $z = $b && true;
                $w = $b ? 1 : 2;
                $p = f() and $b;
            }
            function f(): bool { return true; }
        