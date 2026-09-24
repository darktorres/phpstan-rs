<?php
            class Payload {}
            function f() {
                $cb = fn (Payload $x): Payload => $x;
            }
        