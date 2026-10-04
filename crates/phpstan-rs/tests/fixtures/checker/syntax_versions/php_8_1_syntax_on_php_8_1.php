<?php
enum E
{
    case A;
}

function f(Countable&Traversable $x, DateTime $d = new DateTime()): never
{
    echo 0o17, count($x), $d->format("Y");
    throw new Exception();
}
