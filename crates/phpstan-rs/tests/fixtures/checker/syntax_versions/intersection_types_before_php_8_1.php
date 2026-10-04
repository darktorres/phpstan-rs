<?php
function f(Countable&Traversable $x): int
{
    return count($x);
}
