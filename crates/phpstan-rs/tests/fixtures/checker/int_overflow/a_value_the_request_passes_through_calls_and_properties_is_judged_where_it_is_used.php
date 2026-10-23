<?php
declare(strict_types=1);

function takesInt(int $n): int
{
    return $n;
}

final class Page
{
    public function __construct(public int $number, public int $size)
    {
    }

    /** @param array<mixed> $query */
    public static function fromQuery(array $query): self
    {
        $number = is_numeric($query['page'] ?? null) ? max(0, (int) $query['page']) : 0;
        return new self($number, 50);
    }

    public function offset(): int
    {
        return takesInt($this->number * $this->size);
    }
}

function triple(int $n): int
{
    return takesInt($n * 3);
}

function scaled(int $n, int $by): int
{
    return takesInt($n * $by);
}

function bounded(int $n): int
{
    return takesInt($n * 1000);
}

/** @param list<mixed> $prices */
function capped(array $prices): int
{
    $sum = 0;
    foreach ($prices as $price) {
        $sum += min(1000, max(0, (int) $price));
    }
    return takesInt($sum * 2);
}

/** @param list<mixed> $prices */
function uncapped(array $prices): int
{
    $sum = 0;
    foreach ($prices as $price) {
        $sum += max(0, (int) $price);
    }
    return takesInt($sum);
}

function callers(): void
{
    $page = (int) ($_GET['page'] ?? 1);
    echo triple($page), scaled($page, 0), scaled(1, $page), bounded(5), bounded(count($_GET)), "\n";
    $prices = array_values((array) ($_GET['prices'] ?? []));
    echo capped($prices), uncapped($prices), "\n";
    echo Page::fromQuery($_GET)->offset(), "\n";
}

$_GET = ['page' => '3', 'prices' => ['4', '5']];
callers();
