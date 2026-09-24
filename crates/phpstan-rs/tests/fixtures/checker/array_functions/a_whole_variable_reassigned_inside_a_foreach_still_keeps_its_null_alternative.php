<?php
            use function PHPStan\Testing\assertType;
            /** @param list<string> $rows */
            function f(array $rows): void {
                $file_path = null;
                foreach ($rows as $row) {
                    $file_path = $row;
                }
                assertType('null|string', $file_path);
            }
        