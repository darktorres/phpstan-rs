<?php
        namespace App;
        class MyRule {
            public function check(\PhpParser\Node $node) {
                $node->getAttribute('parent');
            }
        }
        