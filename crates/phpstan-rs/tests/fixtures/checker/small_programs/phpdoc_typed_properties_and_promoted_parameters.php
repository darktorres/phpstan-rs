<?php

class Box
{
    /** @var int */
    public $value;
}

class Point9
{
    /**
     * @param int[] $coords
     */
    public function __construct(public $coords) {}
}

/**
 * @param int $x
 * @return int
 */
function double($x)
{
    return $x * 2;
}

/**
 * @param int[] $items
 * @return int
 */
function sumAll(array $items)
{
    $total = 0;
    foreach ($items as $item) {
        $total += $item;
    }
    return $total;
}

$box = new Box();
$box->value = 42;

$p = new Point9([1, 2, 3]);

echo double(21);
echo sumAll([1, 2, 3]);
