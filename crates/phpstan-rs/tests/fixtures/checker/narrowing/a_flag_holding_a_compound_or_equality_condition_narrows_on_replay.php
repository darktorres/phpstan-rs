<?php
            use function PHPStan\Testing\assertType;
            function f(?int $parentId): void {
                $parentIdIsEmpty = $parentId === null || $parentId === 0;
                if (! $parentIdIsEmpty) {
                    assertType('int<min, -1>|int<1, max>', $parentId);
                }
            }
        