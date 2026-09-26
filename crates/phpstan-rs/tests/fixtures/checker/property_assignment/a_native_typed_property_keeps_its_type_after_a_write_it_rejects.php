<?php
class A
{
    public int $n = 0;
    public ?string $s = null;
    public float $f = 0.0;

    public function write(): void
    {
        $this->n = 'abc';
        \PHPStan\Testing\assertType('int', $this->n);
        $this->s = null;
        \PHPStan\Testing\assertType('null', $this->s);
        $this->f = 3;
        \PHPStan\Testing\assertType('float', $this->f);
        $this->n = 5;
        \PHPStan\Testing\assertType('5', $this->n);
    }
}
