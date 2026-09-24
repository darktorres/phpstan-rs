<?php
            namespace Psl\Str;
            function chunk(string $s): array { return [$s]; }
            function f(string $s): array {
                return namespace\chunk($s);
            }
        