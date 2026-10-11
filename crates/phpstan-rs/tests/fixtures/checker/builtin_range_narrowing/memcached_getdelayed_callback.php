<?php
declare(strict_types=1);

use function PHPStan\Testing\assertType;

final class MemcachedCallbackBox
{
    public int $n = 0;
}

// Control: `getDelayed` runs its callback once per item, and the callable is
// read through the signature, so the count is `int`.
function memcachedCallbackWrites(\Memcached $m, MemcachedCallbackBox $box): void
{
    $box->n = 0;
    $m->getDelayed(['a'], true, static function (\Memcached $client, array $item) use ($box): bool {
        $box->n++;
        return true;
    });
    assertType('int', $box->n);
}
