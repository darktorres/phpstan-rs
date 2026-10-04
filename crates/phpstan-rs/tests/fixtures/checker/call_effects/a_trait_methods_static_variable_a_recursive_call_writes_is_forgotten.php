<?php
namespace CallEffects\TraitMethodStatic;

use function PHPStan\Testing\assertType;

trait Depth
{
    public function depth(int $level): int
    {
        static $deepest = null;
        if ($level === 0) {
            $deepest = 3;
            return 0;
        }
        $deepest = null;
        echo $this->depth($level - 1);
        assertType('mixed', $deepest);
        return 1;
    }
}

final class UsesDepth
{
    use Depth;
}

echo (new UsesDepth())->depth(1);
