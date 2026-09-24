<?php
                function voidFn(): void {}
                function f() {
                    $x = voidFn();
                    $x();
                }
            