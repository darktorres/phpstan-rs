<?php
            use function PHPStan\Testing\assertType;
            /**
             * @return array{seed?: int}
             */
            function parseIt(): array {
                return [];
            }
            function f(bool $a, bool $b, bool $c, bool $d, bool $e, bool $g, bool $h): void {
                /** @var array<string, mixed> */
                $x = [];
                $x = array_merge($x, parseIt());
                if ($a) { $x['a'] = 1; }
                if ($b) { $x['b'] = 2; }
                if ($c) { $x['c'] = 3; }
                if ($d) { $x['d'] = 4; }
                if ($e) { $x['e'] = 5; }
                if ($g) { $x['g'] = 6; }
                if ($h) { $x['h'] = 7; }
                assertType(
                    // `$x`'s open `...` part may already hold any of these
                    // keys, so a branch that writes one only narrows it
                    // to "maybe written".
                    "array{seed?: int, a?: mixed, b?: mixed, c?: mixed, d?: mixed, e?: mixed, g?: mixed, h?: mixed, ...<string, mixed>}",
                    $x
                );
            }
        