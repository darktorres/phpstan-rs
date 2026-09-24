<?php
            class Foo {
                /** @throws void */
                public function bar(): void {
                    throw new \RuntimeException('surprise');
                }
            }
        