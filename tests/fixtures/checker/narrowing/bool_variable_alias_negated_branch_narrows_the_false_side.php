<?php
            class Base1 {}
            function sink1(Base1 $x): void {}
            function f(?Base1 $x): void {
                $isNull = $x === null;
                if (!$isNull) {
                    sink1($x);
                }
            }
        