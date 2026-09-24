<?php
            use function PHPStan\Testing\assertType;
            function fromArray(?string $status_raw, string $other): string {
                if (in_array($status_raw, [$other, 'public'], true)) {
                    assertType('string', $status_raw);
                } else {
                    assertType('string|null', $status_raw);
                }
                $status = in_array($status_raw, [$other, 'public'], true) ? $status_raw : '';
                return $status;
            }
        