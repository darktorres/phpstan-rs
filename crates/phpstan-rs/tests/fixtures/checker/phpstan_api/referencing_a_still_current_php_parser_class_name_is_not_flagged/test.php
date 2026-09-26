<?php
        namespace App;
        class MyRule implements \PHPStan\Rules\Rule {
            public function check() {
                new \PhpParser\Node\Scalar\Int_(1);
            }
        }
        