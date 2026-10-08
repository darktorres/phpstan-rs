<?php
namespace CallEffects\GlobalSites;

use function PHPStan\Testing\assertType;

$plain = 'a';
$incremented = 0;
$matched = null;
$listed = null;
$each = null;
$byRefMethod = null;
$byValueMethod = 3;
$byValuePosition = 'k';
$byRefPosition = null;
$caught = null;
$nested = null;

class Holder
{
    /** @param array<int> $into */
    public function fillTheGivenArray(array &$into): void
    {
        $into = [1];
    }

    /** @param array<int> $into */
    public function store(string $key, array &$into): void
    {
        $into = [1];
    }

    public function countTheGivenNumber(int $from): int
    {
        return $from;
    }
}

$incremented++;
preg_match('/a/', 'a', $matched);
[$listed] = [1];
foreach ([1, 2] as $each) {
}
(new Holder())->fillTheGivenArray($byRefMethod);
(new Holder())->countTheGivenNumber($byValueMethod);
(new Holder())->store($byValuePosition, $byRefPosition);
try {
} catch (\Exception $caught) {
}
$other = ($nested = 5) + 1;

function reads(): void
{
    global $plain, $incremented, $matched, $listed, $each, $byRefMethod, $byValueMethod, $byValuePosition, $byRefPosition, $caught, $nested;
    assertType("'a'", $plain);
    assertType('mixed', $incremented);
    assertType('mixed', $matched);
    assertType('mixed', $listed);
    assertType('mixed', $each);
    assertType('mixed', $byRefMethod);
    assertType('3', $byValueMethod);
    assertType("'k'", $byValuePosition);
    assertType('mixed', $byRefPosition);
    assertType('mixed', $caught);
    assertType('null|5', $nested);
}
