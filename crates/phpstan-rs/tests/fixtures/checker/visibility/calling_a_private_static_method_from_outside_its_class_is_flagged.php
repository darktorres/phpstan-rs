<?php
            class Foo {
                private static function secret(): int { return 1; }
            }
            function test(): int {
                return Foo::secret();
            }
        