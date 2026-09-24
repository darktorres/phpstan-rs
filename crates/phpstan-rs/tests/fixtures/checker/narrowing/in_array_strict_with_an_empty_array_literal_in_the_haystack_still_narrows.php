<?php
            function needsString(string $s): void {}
            function f(array|string|null $raw): void {
                if (! in_array($raw, [null, '0', '', []], true)) {
                    if (! is_array($raw)) {
                        needsString($raw);
                    }
                }
            }
        