<?php
            use function PHPStan\Testing\assertType;
            /** @param array<string, mixed> $s */
            function f(string $a, array $s): void {
                switch ($a) {
                    case 'a':
                    case 'b':
                        assertType("'a'|'b'", $a);
                        break;
                    case 'moderate':
                        $s['p'] = [];
                        // no break
                    case 'validate':
                        assertType("'moderate'|'validate'", $a);
                        if (! isset($s['p']) || ! is_array($s['p'])) {
                            $s['p'] = [];
                        }
                        break;
                }
            }
        