<?php
            use function PHPStan\Testing\assertType;
            /**
             * @return array{a: string}
             */
            function parseIt(): array {
                return ['a' => 'x'];
            }
            function f(): void {
                /** @var array<string, mixed> */
                $page = [];
                $page['a'] = 1;
                $page = array_merge($page, parseIt());
                assertType("array{a: string, ...}", $page);
            }
        