<?php
            class Foo {
                private static function secret(): void {}
            }
            function test(): void {
                $h = Foo::secret(...);
            }
        