<?php
            use function PHPStan\Testing\assertType;
            function f(bool $a, bool $b, bool $c, bool $d, bool $e, bool $g, bool $h): void {
                /** @var array<string, mixed> */
                $x = [];
                if ($a) { $x['a'] = 1; }
                if ($b) { $x['b'] = 2; }
                if ($c) { $x['c'] = 3; }
                if ($d) { $x['d'] = 4; }
                if ($e) { $x['e'] = 5; }
                if ($g) { $x['g'] = 6; }
                if ($h) { $x['h'] = 7; }
                assertType('array<string, mixed>', $x);
            }
        