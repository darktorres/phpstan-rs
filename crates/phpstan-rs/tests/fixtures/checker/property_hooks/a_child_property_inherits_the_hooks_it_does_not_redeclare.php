<?php
class P
{
    public string $name {
        set(string $value) {
            $this->name = strtolower($value);
        }
    }
}

class C extends P
{
    public string $name {
        get => strtoupper(parent::$name::get());
    }
}

class D extends P
{
    public string $name;
}

function f(C $c, D $d): void
{
    $c->name = 'X';
    $d->name = 'Y';
    \PHPStan\Testing\assertType('string', $c->name);
}

class Plain
{
    public string $n = 'a';
}

class Fixed extends Plain
{
    public string $n {
        get => 'fixed';
    }
}
