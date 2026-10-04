<?php
namespace CallEffects\MethodStatic;

use function PHPStan\Testing\assertType;

class Base
{
    public function depth(int $level): int
    {
        static $deepest = null;
        if ($level === 0) {
            $deepest = 3;
            return 0;
        }
        $deepest = null;
        echo (new Base())->depth($level - 1);
        assertType('mixed', $deepest);
        return 1;
    }
}

final class Child extends Base
{
}

echo (new Child())->depth(1);
echo (new Base())->depth(1);
