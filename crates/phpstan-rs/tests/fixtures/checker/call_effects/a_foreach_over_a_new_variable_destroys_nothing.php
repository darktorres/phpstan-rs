<?php
namespace CallEffects\ForeachDestroysNothing;

use function PHPStan\Testing\assertType;

class Noisy
{
    public function __destruct()
    {
        $name = $_GET['callback'];
        $name();
    }
}

function rewrite(): void
{
    global $items;
    $items = 'rewritten';
}

$items = [1, 2];
foreach ($items as $key => $item) {
    echo strlen('a');
}
assertType('array{0: 1, 1: 2}', $items);

$held = new Noisy();
$held = 'now a string';
assertType("array{0?: 1, 1?: 2}|'rewritten'", $items);
