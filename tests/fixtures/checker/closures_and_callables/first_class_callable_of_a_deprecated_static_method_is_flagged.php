<?php
            class Foo {
                /** @deprecated */
                public static function oldMethod(): void {}
            }
            function test(): void {
                $h = Foo::oldMethod(...);
            }
        