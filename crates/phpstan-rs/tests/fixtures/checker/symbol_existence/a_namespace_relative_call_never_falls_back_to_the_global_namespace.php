<?php
            function chunk(string $s): array { return [$s]; }
            namespace Psl\Str {
                function f(string $s): array {
                    return namespace\chunk($s);
                }
            }
        