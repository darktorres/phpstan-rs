<?php
namespace CallEffects\StableProperties\Elements;

use function PHPStan\Testing\assertType;

final class Table
{
    /** @var array<string, int|null> */
    public array $rows;

    /** @var array<string, int|null> */
    public array $edited;

    /** @param array<string, int|null> $rows */
    public function __construct(array $rows)
    {
        $this->rows = $rows;
        $this->edited = $rows;
    }

    public function edit(string $key): void
    {
        $this->edited[$key] = null;
    }
}

function anElementOfAStableProperty(Table $table, callable $callback): void
{
    if ($table->rows['a'] !== null) {
        $callback();
        assertType('int', $table->rows['a']);
    }
}

function anElementOfAWrittenProperty(Table $table, callable $callback): void
{
    if ($table->edited['a'] !== null) {
        $callback();
        assertType('int|null', $table->edited['a']);
    }
}
