<?php
            function needsArray(array $x): void {}
            function test(mixed $value): void {
                if (is_iterable($value)) {
                    if (\is_array($value)) {
                        needsArray($value);
                    }
                }
            }
        