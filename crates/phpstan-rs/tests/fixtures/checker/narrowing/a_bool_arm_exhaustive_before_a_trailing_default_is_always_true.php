<?php
            function f(bool $b): string {
                return match ($b) {
                    true => 'a',
                    false => 'b',
                    default => 'unreachable',
                };
            }
        