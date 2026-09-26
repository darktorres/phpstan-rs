<?php
/** @extends \ArrayIterator<int, string> */
class Child extends \ArrayIterator
{
    #[\ReturnTypeWillChange]
    public function current()
    {
        return parent::current();
    }

    public function key(): string|int|null
    {
        \PHPStan\Testing\assertType('int|null', parent::key());
        return parent::key();
    }
}

class Counted implements \Countable
{
    #[\ReturnTypeWillChange]
    public function count()
    {
        return 0;
    }
}

function f(Child $c, Counted $n): void
{
    \PHPStan\Testing\assertType('string', $c->current());
    \PHPStan\Testing\assertType('int<0, max>', $n->count());
}
