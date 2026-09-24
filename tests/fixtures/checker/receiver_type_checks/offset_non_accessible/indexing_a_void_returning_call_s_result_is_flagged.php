<?php
                function voidFn(): void {}
                function f() {
                    $x = voidFn();
                    echo $x['a'];
                }
            