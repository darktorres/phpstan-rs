<?php
namespace CallEffects\HeldCallable;

use function PHPStan\Testing\assertType;

final class Suspension
{
    public bool $pending = false;

    /** @param \Closure(): void $run */
    public function __construct(private \Closure $run)
    {
    }

    public function suspend(): void
    {
        $this->pending = true;
        ($this->run)();
        assertType('bool', $this->pending);
    }

    public function resume(): void
    {
        $this->pending = false;
    }
}

$suspension = null;
$suspension = new Suspension(static function () use (&$suspension): void {
    $suspension?->resume();
});
$suspension->suspend();
