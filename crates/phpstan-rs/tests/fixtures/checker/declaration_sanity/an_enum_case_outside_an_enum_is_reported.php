<?php
namespace EnumCaseOutsideEnum;

class Foo
{
    case Active;
}

interface Bar
{
    case Active;
}

trait Baz
{
    case Active;
}

class BazConsumer
{
    use Baz;
}

enum Qux
{
    case Active;
}

function f(): object
{
    return new class {
        case Active;
    };
}
