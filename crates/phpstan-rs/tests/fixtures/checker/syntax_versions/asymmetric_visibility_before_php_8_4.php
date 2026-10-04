<?php
class A
{
    public private(set) int $x = 1;

    public function __construct(public protected(set) int $y)
    {
    }
}
