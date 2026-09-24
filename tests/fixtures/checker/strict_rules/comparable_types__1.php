<?php
function f(int $i, float $f, string $s, ?int $maybe, bool $b, \DateTimeImmutable $d, \DateTime $e, mixed $m): void {
    var_dump($i < $f);
    var_dump($s >= 'a');
    var_dump($d < $e);
    var_dump($i < $s);
    var_dump($maybe > 1);
    var_dump($b <=> true);
    var_dump($m < 1);
    var_dump([$i, $s] < [$f, 'x']);
    var_dump([$i, $s] < [$s, 'x']);
    var_dump($d > 5);
}
