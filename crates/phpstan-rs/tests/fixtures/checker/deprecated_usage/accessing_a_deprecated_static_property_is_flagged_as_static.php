<?php
            class Foo {
                /** @deprecated */
                public static int $oldProp = 1;
            }
            function f(): int {
                return Foo::$oldProp;
            }
        