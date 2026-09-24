<?php
                function voidFn(): void {}
                function f() {
                    foreach (voidFn() as $v) {
                    }
                }
            