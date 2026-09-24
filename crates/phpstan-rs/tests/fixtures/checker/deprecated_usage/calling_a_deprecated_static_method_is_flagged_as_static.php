<?php
            class Foo {
                /** @deprecated */
                public static function oldMethod(): void {}
            }
            function f(): void {
                Foo::oldMethod();
            }
        