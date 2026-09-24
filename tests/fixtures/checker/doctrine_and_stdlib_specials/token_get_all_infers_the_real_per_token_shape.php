<?php
            /** @return list<array{path: string, line: int}> */
            function findBareCallSites(string $source): array {
                $hits = [];
                $tokens = token_get_all($source);
                $n = count($tokens);
                for ($i = 0; $i < $n; $i++) {
                    $tok = $tokens[$i];
                    if (! is_array($tok) || $tok[0] !== T_STRING) {
                        continue;
                    }
                    $hits[] = [
                        'path' => 'x',
                        'line' => $tok[2],
                    ];
                }
                return $hits;
            }
        