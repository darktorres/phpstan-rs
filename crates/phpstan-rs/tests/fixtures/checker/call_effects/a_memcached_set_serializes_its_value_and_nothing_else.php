<?php
namespace CallEffects\MemcachedSet;

use function PHPStan\Testing\assertType;

final class Tally
{
    public int $sets = 0;
}

final class Quiet
{
    public function __serialize(): array
    {
        return [];
    }

    /** @return list<string> */
    public function __sleep(): array
    {
        return [];
    }
}

function storesAQuietValue(\Memcached $cache, Tally $tally): void
{
    $tally->sets = 0;
    $cache->set('key', new Quiet());
    assertType('0', $tally->sets);
}
