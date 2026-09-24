<?php
                function f(int $x) {
                    foreach ($x as $v) {
                        $v->realMethodThatDoesNotMatterHere();
                    }
                }
            