<?php

function takesInt(int $n): int
{
    return $n;
}

class Counter
{
    public int $total = 0;
    public static int $shared = 0;

    public function add(): void
    {
        $page = (int) ($_GET['page'] ?? 1);
        $this->total = $page * 100;
        self::$shared = $page * 100;
        $this->total += $page * 100;
    }
}

function sinks(): int
{
    $page = (int) ($_GET['page'] ?? 1);
    $offset = $page * 20;
    $taken = takesInt($offset);
    $char = chr($offset);
    $rows = [];
    $rows[$offset] = 1;
    $keyed = [$offset => 1];
    $text = 'abc';
    $letter = $text[$offset] ?? '';
    $found = $rows[$offset] ?? 0;
    $low = $offset % 7;
    $mask = $offset & 1;
    $shifted = $offset << 1;
    $flipped = ~$offset;
    $cast = (int) $offset;
    $value = intval($offset);
    settype($offset, 'int');
    $printed = sprintf('%d', $page * 20);
    $next = $page;
    $next++;
    $taken = takesInt($next);
    return $page + 1;
}

function boundedSink(): int
{
    $max = PHP_INT_MAX;
    return takesInt($max + 1);
}

$_GET = ['page' => '3'];
(new Counter())->add();
echo sinks(), "\n";
