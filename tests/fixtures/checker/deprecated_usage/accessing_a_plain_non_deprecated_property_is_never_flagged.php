<?php
            class Foo {
                public int $niceProp;
            }
            function f(Foo $foo): int {
                return $foo->niceProp;
            }
        