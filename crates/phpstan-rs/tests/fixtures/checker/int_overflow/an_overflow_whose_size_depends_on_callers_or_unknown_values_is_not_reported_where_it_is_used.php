<?php

function takesInt(int $n): int
{
    return $n;
}

/** @param int $n */
function takesDocInt($n): int
{
    return 1;
}

class Pager
{
    public int $perPage = 10;

    public function offset(int $page): int
    {
        return takesInt($page * $this->perPage);
    }

    public function first(): int
    {
        return $this->perPage * 3;
    }
}

function fromCaller(int $page): int
{
    return $page + 1;
}

function fromUnknown(mixed $value): int
{
    $n = (int) $value;
    return takesInt($n * 2);
}

function docOnly(): int
{
    $page = (int) ($_GET['page'] ?? 1);
    return takesDocInt($page * 2);
}

function guardedByRange(): int
{
    $page = (int) ($_GET['page'] ?? 1);
    if ($page < 1 || $page > 1000) {
        return 0;
    }
    return takesInt($page * 20);
}

function divideByLiteral(): int
{
    $page = (int) ($_GET['page'] ?? 1);
    return intdiv($page, 3) + intdiv(PHP_INT_MIN, max(1, (int) ($_GET['by'] ?? 1))) * 0;
}

function intoFloat(): float
{
    $page = (int) ($_GET['page'] ?? 1);
    return $page * 20;
}

$_GET = ['page' => '3'];
echo (new Pager())->offset(2), (new Pager())->first(), fromCaller(1), fromUnknown('4'), docOnly(), guardedByRange(), intoFloat(), "\n";
