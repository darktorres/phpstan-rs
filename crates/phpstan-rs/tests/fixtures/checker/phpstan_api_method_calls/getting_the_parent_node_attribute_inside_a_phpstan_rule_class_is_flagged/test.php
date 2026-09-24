<?php
        namespace App;
        class MyRule implements \PHPStan\Rules\Rule {
            public function check(\PhpParser\Node $node) {
                $node->getAttribute('parent');
            }
        }
        