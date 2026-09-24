<?php
            use function PHPStan\Testing\assertType;
            function f(bool $isUserFound, ?string $email): void {
                $skipMail = ! $isUserFound || $email === null || $email === '';
                if (! $skipMail) {
                    assertType('non-empty-string', $email);
                }
            }
        