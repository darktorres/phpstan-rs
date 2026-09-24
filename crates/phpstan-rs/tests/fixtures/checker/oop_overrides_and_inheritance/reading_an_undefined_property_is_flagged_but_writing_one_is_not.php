<?php
            class Box {
                public int $known = 1;
            }
            function f(Box $box) {
                echo $box->known;
                echo $box->totallyMadeUp;
                $box->dynamicWrite = 5;
                [$box->a, $box->b] = [1, 2];
            }
        