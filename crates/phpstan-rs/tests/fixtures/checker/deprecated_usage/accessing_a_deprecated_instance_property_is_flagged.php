<?php
            class Foo {
                /** @deprecated */
                public int $oldProp;
            }
            function f(Foo $foo): int {
                return $foo->oldProp;
            }
        