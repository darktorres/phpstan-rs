<?php
            class Thing {
                public function __toString(): string { return "x"; }
            }
            function f(Thing $t) {
                echo (string) $t;
            }
        