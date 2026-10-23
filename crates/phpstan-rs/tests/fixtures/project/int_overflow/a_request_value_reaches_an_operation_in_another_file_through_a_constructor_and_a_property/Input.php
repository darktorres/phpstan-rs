<?php
declare(strict_types=1);

namespace App;

final class SearchInput
{
    public function __construct(public int $pageNumber)
    {
    }

    /** @param array<mixed> $raw */
    public static function fromArray(array $raw): self
    {
        $pageNumber = $raw['pageNumber'] ?? null;
        return new self(pageNumber: is_numeric($pageNumber) ? max(0, (int) $pageNumber) : 0);
    }
}
