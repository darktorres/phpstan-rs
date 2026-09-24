<?php
            use function PHPStan\Testing\assertType;
            function f(?string $exif, ?string $exif2): void {
                if ((bool) $exif || (bool) $exif2) {
                    if ((bool) $exif2) {
                    } else {
                        assertType('non-falsy-string', $exif);
                    }
                }
            }
        