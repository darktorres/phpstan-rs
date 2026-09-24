<?php
const LIMIT = 10;
define('OTHER', 20);
class Box {
    public const SIZE = 3;
    public int $count = 5;
    public int $zero = 0;
    public function __construct(private int $max = 7) {}
    public function run(int $n, int $flags): int {
        $x = 42;
        $y = $n * 2;
        $z = $flags & 0x10;
        $cmp = $n > 100;
        $t = $n ? 3 : self::SIZE;
        $s = '12';
        $neg = -1;
        $grouped = (5);
        strlen('abc', 8);
        $m = match ($n) { 4 => 'four', default => 'other' };
        switch ($n) { case 9: break; }
        $ok = $n + 1;
        return 60;
    }
}
if (true) {
    class Nested { public int $limit = 99; }
}
