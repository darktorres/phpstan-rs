<?php
            function test(int $i, float $f, string $s, bool $b, ?string $n): void {
                $a = (int) $s;
                $c = (float) $i;
                $e = (string) $i;
                $g = (string) $f;
                $h = (int) $b;
                $j = (string) $n;
            }
        