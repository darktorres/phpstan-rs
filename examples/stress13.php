<?php

class DomainError extends \Exception
{
    public function code(): int
    {
        return 42;
    }
}

function needsInt(int $x): int
{
    return $x;
}

/**
 * @param int[] $nums
 */
function sumEach(array $nums): int
{
    $total = 0;
    foreach ($nums as $n) {
        $total += needsInt($n);
    }
    return $total;
}

function handle(): int
{
    try {
        throw new DomainError("bad");
    } catch (DomainError $e) {
        return needsInt($e->code());
    }
}

/**
 * @param int[] $pair
 */
function addPair(array $pair): int
{
    [$a, $b] = $pair;
    return needsInt($a) + needsInt($b);
}

echo sumEach([1, 2, 3]);
echo handle();
echo addPair([1, 2]);
