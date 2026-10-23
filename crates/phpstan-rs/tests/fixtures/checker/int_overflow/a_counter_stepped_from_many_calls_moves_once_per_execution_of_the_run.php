<?php
declare(strict_types=1);

final class Aliases
{
    private int $counter = 0;

    private function next(string $table): string
    {
        return $table . "_" . $this->counter++;
    }

    public function part0(): string
    {
        return $this->next("t0");
    }

    public function part1(): string
    {
        return $this->next("t1");
    }

    public function part2(): string
    {
        return $this->next("t2");
    }

    public function part3(): string
    {
        return $this->next("t3");
    }

    public function part4(): string
    {
        return $this->next("t4");
    }

    public function part5(): string
    {
        return $this->next("t5");
    }

    public function part6(): string
    {
        return $this->next("t6");
    }

    public function part7(): string
    {
        return $this->next("t7");
    }

    public function part8(): string
    {
        return $this->next("t8");
    }

    public function part9(): string
    {
        return $this->next("t9");
    }

    public function part10(): string
    {
        return $this->next("t10");
    }

    public function part11(): string
    {
        return $this->next("t11");
    }

    public function part12(): string
    {
        return $this->next("t12");
    }

    public function part13(): string
    {
        return $this->next("t13");
    }

    public function part14(): string
    {
        return $this->next("t14");
    }

    public function part15(): string
    {
        return $this->next("t15");
    }

    public function part16(): string
    {
        return $this->next("t16");
    }

    public function part17(): string
    {
        return $this->next("t17");
    }

    public function part18(): string
    {
        return $this->next("t18");
    }

    public function part19(): string
    {
        return $this->next("t19");
    }

}

$aliases = new Aliases();
echo $aliases->part0(), " ", $aliases->part19(), "\n";
