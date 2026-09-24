<?php
            use function PHPStan\Testing\assertType;
            function f(bool $isUserFound, ?string $email, ?string $other): void {
                $skipMail = ! $isUserFound || $email === null;
                $email = $other;
                if (! $skipMail) {
                    assertType('string|null', $email);
                }
            }
        