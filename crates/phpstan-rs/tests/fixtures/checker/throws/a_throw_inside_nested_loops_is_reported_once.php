<?php

declare(strict_types=1);

final class DuplicateSlug extends \Exception
{
}

final class Pages
{
    /**
     * @param array<string, list<string>> $groups
     * @return array<string, string>
     */
    public function bySlug(array $groups): array
    {
        $pages = [];
        foreach ($groups as $group => $slugs) {
            foreach ($slugs as $slug) {
                if (isset($pages[$slug])) {
                    throw new DuplicateSlug($group);
                }
                $pages[$slug] = $group;
            }
        }

        return $pages;
    }
}
