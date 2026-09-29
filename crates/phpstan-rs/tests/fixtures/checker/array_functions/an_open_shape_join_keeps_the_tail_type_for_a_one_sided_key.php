<?php
            use function PHPStan\Testing\assertType;
            enum Section { case Categories; case Tags; }
            /** @param array<string, mixed> $parsed */
            function f(array $parsed): void {
                $page = array_merge(['items' => 1], $parsed);
                if (! isset($page['section'])) {
                    $page['section'] = Section::Categories;
                }
                assertType('array{items: mixed, section: mixed~null, ...}', $page);
            }
        