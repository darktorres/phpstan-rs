<?php
            function f(int $i, int|string $m, string $s, object $o): void {
                $a = new $i();
                $b = new $m();
                $c = new $s();
                $d = new $o();
            }
        