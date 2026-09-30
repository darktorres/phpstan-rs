<?php
            use function PHPStan\Testing\assertType;
            function fromArray(?string $status_raw): string {
                if (in_array($status_raw, ['public', 'private'])) {
                    assertType("'public'|'private'", $status_raw);
                }
                $status = in_array($status_raw, ['public', 'private']) ? $status_raw : '';
                return $status;
            }
        
