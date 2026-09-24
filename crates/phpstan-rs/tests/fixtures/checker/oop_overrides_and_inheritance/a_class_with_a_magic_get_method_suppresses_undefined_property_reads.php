<?php
            class Box {
                public function __get($name) { return null; }
            }
            function f(Box $box) {
                echo $box->anything;
            }
        