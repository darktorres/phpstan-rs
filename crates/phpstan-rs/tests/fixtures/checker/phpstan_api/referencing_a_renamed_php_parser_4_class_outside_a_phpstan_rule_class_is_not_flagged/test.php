<?php
        namespace App;
        class MyRule {
            public function check() {
                new \PhpParser\Node\Scalar\LNumber(1);
            }
        }
        