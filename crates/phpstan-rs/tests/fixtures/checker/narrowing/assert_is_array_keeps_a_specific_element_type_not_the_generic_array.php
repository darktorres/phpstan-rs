<?php
            class T {
                /** @param array<string>|int $x */
                function f($x): array {
                    self::assertIsArray($x);
                    return $x;
                }
                static function assertIsArray($x): void {}
            }
        