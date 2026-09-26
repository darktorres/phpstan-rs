<?php
        namespace App;
        class MyRule implements \PHPStan\Rules\Rule {
            public function check($x) {
                new \ReflectionClass($x);
            }
        }
        