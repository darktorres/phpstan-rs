<?php
            namespace Amp\Future {
                function awaitAll(array $futures): array { return $futures; }
            }
            namespace App {
                use Amp\Future;
                function run(): array {
                    return Future\awaitAll([]);
                }
            }
        