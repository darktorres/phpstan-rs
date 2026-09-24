<?php
            interface HasBag1 {}
            class RealBag1 implements HasBag1 {
                public string $bag = '';
            }
            function f(HasBag1 $x): void {
                if (property_exists($x, 'bag')) {
                    echo $x->bag;
                }
            }
        