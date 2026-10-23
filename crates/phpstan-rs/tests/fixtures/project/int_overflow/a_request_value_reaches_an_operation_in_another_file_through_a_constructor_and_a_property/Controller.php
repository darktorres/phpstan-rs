<?php
declare(strict_types=1);

namespace App;

final class SearchController
{
    private const int PAGE_SIZE = 300;

    /**
     * @param list<string> $lines
     * @return list<string>
     */
    public function page(array $lines): array
    {
        $input = SearchInput::fromArray($_GET);
        return array_slice($lines, $input->pageNumber * self::PAGE_SIZE, self::PAGE_SIZE);
    }

    /**
     * @param list<string> $lines
     * @return list<string>
     */
    public function first(array $lines): array
    {
        $input = new SearchInput(2);
        return array_slice($lines, $input->pageNumber * self::PAGE_SIZE, self::PAGE_SIZE);
    }
}
