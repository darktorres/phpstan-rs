<?php
            final class Point { public int $x = 0; }
            function test(int $a, array $list, string $s, Point $p): void {
                $a;
                $a + 1;
                $a === 2;
                $list['key'];
                $s[0];
                $p->x;
                isset($list['k']);
                empty($s);
                [$a, 'b' => $s];
                (int) $s;
                -$a;
                $p instanceof Point;
                PHP_EOL;
                Point::class;
                fn() => 1;
                "x{$a}y";
                $list ?? null;
            }
        