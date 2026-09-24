<?php
            namespace App;
            class Known { const A = 1; }
            /**
             * @param string[optional] $a
             * @param Closure(int): string $b
             * @param Known::MISSING $c
             * @param self $d
             * @param int<5, 1> $e
             * @param int&string $f
             * @param array<int, string, bool> $g
             * @param non-empty-foo $h
             * @param-out Unknown::X $i
             * @return list<int, string>
             */
            function f($a, $b, $c, $d, $e, $f, $g, $h, &$i) { return []; }
        