<?php
class Foo
{
    public readonly int $b;

    public function __construct(Foo $other)
    {
        $other->b = 1;
        $this->b = 2;
    }

    public function __clone()
    {
        $this->b = 3;
        $this->b = 4;
    }
}

class Bar
{
    public readonly int $c;

    public function __clone()
    {
        if (rand(0, 1)) {
            $this->c = 1;
        }
        $this->c = 2;
    }
}
