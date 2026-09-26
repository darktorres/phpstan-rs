<?php
        namespace App;
        class MyCallback implements \PHPStan\Type\TypeTraverserCallable {
            public function run($x) {
                if ($x instanceof \PHPStan\Type\StringType) {
                }
            }
        }
        