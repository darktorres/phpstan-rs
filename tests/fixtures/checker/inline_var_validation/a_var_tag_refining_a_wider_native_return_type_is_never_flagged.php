<?php
            function f(): void {
                /** @var array{a: int} $x */
                $x = returnsArray();
            }
            function returnsArray(): array { return ['a' => 1]; }
        