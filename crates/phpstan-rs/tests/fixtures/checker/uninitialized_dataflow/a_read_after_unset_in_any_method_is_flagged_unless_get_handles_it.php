<?php

class P
{
    public int $x;
    public int $d = 1;
    public ?int $u;

    public function __construct(public string $pp)
    {
        $this->x = 1;
        $this->u = null;
    }

    public function m(): void
    {
        unset($this->x);
        echo $this->x;
        unset($this->d, $this->pp);
        echo $this->d, $this->pp;
        $this->x = 2;
        echo $this->x;
        echo $this->u;
    }

    public function probe(): bool
    {
        unset($this->x);
        return isset($this->x);
    }
}

class G
{
    public int $x = 1;

    public function __get(string $n): int
    {
        return 1;
    }

    public function m(): int
    {
        unset($this->x);
        return $this->x;
    }
}
