<?php
            use function PHPStan\Testing\assertType;
            function transactional(callable $cb): void {
                $cb();
            }
            function f(bool $userAnonymous, string $anonymousId): void {
                if ($userAnonymous) {
                    $savedAnonymousId = "abc";
                } else {
                    $savedAnonymousId = null;
                }
                transactional(function () use ($userAnonymous, $anonymousId, $savedAnonymousId): void {
                    if ($userAnonymous && $anonymousId !== $savedAnonymousId) {
                        assertType("'abc'", $savedAnonymousId);
                    }
                });
            }
        