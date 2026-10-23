<?php
declare(strict_types=1);

final class Element
{
    public function __construct(public int $value)
    {
    }
}

function add(Element $f, Element $g): Element
{
    return new Element($f->value + $g->value);
}

function double(Element $x): Element
{
    return add($x, $x);
}

function quadruple(Element $x): Element
{
    return add(double($x), double($x));
}

$x = new Element((int) ($_GET['x'] ?? 0) & 0xffff);
echo quadruple($x)->value, "\n";
