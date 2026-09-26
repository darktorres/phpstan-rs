<?php
        namespace App;
        class MyRule {
            public function check($x) {
                is_a($x, 'Foo');
            }
        }
        