<?php
            /** @return list<string> */
            function f(string $pattern): array {
                $paths = glob($pattern);
                return $paths !== false ? $paths : [];
            }
        