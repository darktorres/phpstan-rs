<?php
            class Foo {}
            function f() {
                $x = null;
                while (rand(0, 1)) {
                    totallyUndefinedFunction();
                    $x = new Foo();
                }
            }
        