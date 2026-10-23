<?php
declare(strict_types=1);

function takesInt(int $n): int
{
    return $n;
}

/** @param list<int<0, 255>> $bytes */
function carried(array $bytes): int
{
    $carry = 0;
    foreach ($bytes as $byte) {
        $carry = ($byte + $byte + $carry) >> 8;
    }
    return takesInt($carry + 1);
}

/** @param array<string, list<string>> $graph */
function depth(array $graph, string $node): int
{
    $most = 0;
    foreach ($graph[$node] ?? [] as $next) {
        $most = max($most, depth($graph, $next) + 1);
    }
    return $most;
}

final class Ring
{
    private int $slot = 0;

    public function next(): int
    {
        $this->slot = ($this->slot + 1) % 1000;
        return takesInt($this->slot * 1000);
    }
}

function skipped(string $text, int $from): int
{
    $at = $from;
    while (($text[$at] ?? '') === ' ') {
        $at++;
    }
    return takesInt($at + 1);
}

function hashed(string $text, int $seed): int
{
    $hash = $seed;
    foreach (str_split($text) as $char) {
        $hash = $hash * 31 + ord($char);
    }
    return takesInt($hash);
}

echo depth(['a' => ['b'], 'b' => []], 'a'), (new Ring())->next(), "\n";
echo carried([255, 255, 1]), skipped('  x', 0), "\n";
echo hashed('ab', (int) ($_GET['seed'] ?? 7)), "\n";
