<?php
            function test(bool $cond): void {
                try {
                    if ($cond) {
                        throw new \RuntimeException('x');
                    }
                } catch (\SomeUnresolvableInterface|\RuntimeException $e) {
                    throw $e;
                }
            }
        