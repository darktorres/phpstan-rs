<?php

class NotFoundException extends \Exception {}

function classify(int $n): string
{
    switch ($n) {
        case 1:
        case 2:
            return "low";
        case 3:
            return "mid";
        default:
            return "high";
    }
}

function render(int $id): string
{
    $name = "widget";
    $sql = <<<SQL
        SELECT *
        FROM items
        WHERE id = {$id}
        SQL;

    $raw = <<<'RAW'
    literal $name stays $literal
    RAW;

    try {
        if ($id < 0) {
            throw new NotFoundException("bad id: $id");
        }
        return $sql . $raw;
    } catch (NotFoundException | \RuntimeException $e) {
        return "error: " . $e->getMessage();
    } finally {
        echo "cleanup\n";
    }
}

echo classify(2);
echo render(5);
