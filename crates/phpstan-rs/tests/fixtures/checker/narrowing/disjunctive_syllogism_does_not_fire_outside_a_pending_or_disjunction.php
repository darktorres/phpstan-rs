<?php
            use function PHPStan\Testing\assertType;
            function f(?string $exif, ?string $exif2): void {
                if ((bool) $exif2) {
                } else {
                    // No enclosing `||` pushed anything - $exif stays
                    // whatever it already was, not narrowed to non-null.
                    assertType('string|null', $exif);
                }
            }
        