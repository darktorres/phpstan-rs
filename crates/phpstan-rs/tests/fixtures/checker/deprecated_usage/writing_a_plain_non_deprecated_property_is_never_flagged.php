<?php
            class Foo {
                public int $niceProp;
            }
            function f(Foo $foo): void {
                $foo->niceProp = 1;
            }
        