<?php
            use function PHPStan\Testing\assertType;
            /**
             * @param list<int> $tags
             * @param list<int> $custom
             */
            function f(bool $hasTags, bool $hasCustom, array $tags, array $custom): void {
                $imageIdsForFilter = [];
                if ($hasTags) {
                    $imageIdsForFilter['tags'] = $tags;
                }
                if ($hasCustom) {
                    $imageIdsForFilter['custom'] = $custom;
                }
                if ($imageIdsForFilter !== []) {
                    $first = reset($imageIdsForFilter);
                    assertType('list<int>', $first);
                }
            }
        