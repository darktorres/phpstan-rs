<?php
            use function PHPStan\Testing\assertType;
            function f(string $k, int $v): void {
                $r = ['qs' => ['q' => 'x']];
                $r['qs']['z'] = 1;
                $r[$k] = $v;
                assertType("array{q: 'x', z: 1}|int", $r['qs']);
            }
        