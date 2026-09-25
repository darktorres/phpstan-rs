<?php
            use function PHPStan\Testing\assertType;
            /**
             * @return array{b?: string}
             */
            function parseIt(): array {
                return [];
            }
            function f(): void {
                /** @var array<string, mixed> */
                $page = [];
                $page['a'] = 1;
                $page = array_merge($page, parseIt());
                assertType("array{a: 1, b?: string, ...}", $page);
            }
        