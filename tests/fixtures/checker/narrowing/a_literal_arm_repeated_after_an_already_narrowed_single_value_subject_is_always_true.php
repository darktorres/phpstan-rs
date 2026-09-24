<?php
            function f(): string {
                $x = 1;
                return match ($x) {
                    1 => 'always true but followed by default',
                    default => 'unreachable',
                };
            }
        