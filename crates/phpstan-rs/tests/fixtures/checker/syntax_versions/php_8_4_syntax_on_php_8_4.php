<?php
class A
{
    public private(set) int $x = 1;

    public function f(): int
    {
        return 1;
    }
}

echo new A()->f();
