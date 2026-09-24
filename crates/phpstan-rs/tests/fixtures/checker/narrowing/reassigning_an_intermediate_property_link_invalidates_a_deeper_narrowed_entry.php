<?php
            class Foo {
                /** @var array<string, mixed> */
                public array $bar = [];
            }
            function needsInt(int $n): void {}
            function handle(Foo $foo): void {
                if (is_int($foo->bar['k'])) {
                    $foo->bar = ['k' => 'newstring'];
                    needsInt($foo->bar['k']);
                }
            }
        