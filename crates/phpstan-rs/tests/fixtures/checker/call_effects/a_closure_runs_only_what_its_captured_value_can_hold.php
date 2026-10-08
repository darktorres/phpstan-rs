<?php
namespace CallEffects\CapturedHolders;

use function PHPStan\Testing\assertType;

final class Box
{
    public ?string $name = null;
}

final class Plain
{
    public function __toString(): string
    {
        return 'plain';
    }
}

function aCapturedArrayRunsNoCode(Box $box): void
{
    $names = ['a', 'b'];
    $join = static function () use ($names): string {
        return implode(',', $names);
    };
    if ($box->name !== null) {
        $join();
        assertType('string', $box->name);
    }
}
