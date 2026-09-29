<?php
namespace App;
use function PHPStan\Testing\assertType;
function outer(): void
{
    function inner(float $x): int { return 1; }
    class Inner { public function v(): int { return 1; } }
}
try { function inTry(): string { return ''; } } catch (\Exception $e) {}
outer();
assertType('int', inner(1.0));
assertType('int', (new Inner())->v());
assertType('string', inTry());
