<?php
            class Foo {
                /** @deprecated */
                public int $oldProp;
            }
            function f(Foo $foo): void {
                $foo->oldProp = 1;
            }
        