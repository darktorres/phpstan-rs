<?php
declare(strict_types=1);

use function PHPStan\Testing\assertType;

final class RedisGetCounter
{
    public int $n = 0;

    public function __serialize(): array
    {
        $this->n++;
        return [];
    }
}

// Control: `get` runs no user code under the default serializer, and the
// table says so precisely.
function redisGetRunsNothingByDefault(\Redis $r, RedisGetCounter $c): void
{
    $c->n = 0;
    $r->get('k');
    assertType('0', $c->n);
}
