<?php
class A
{
    public int $p = 1;

    public function f(): int
    {
        return 1;
    }
}

echo new A()->f();
echo new A()->p;
echo (new A())->f();
