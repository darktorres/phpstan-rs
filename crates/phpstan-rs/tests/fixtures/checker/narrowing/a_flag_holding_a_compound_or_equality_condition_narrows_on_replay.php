<?php
            use function PHPStan\Testing\assertType;
            function f(?int $parentId): void {
                $parentIdIsEmpty = $parentId === null || $parentId === 0;
                if (! $parentIdIsEmpty) {
                    assertType('int', $parentId);
                }
            }
        