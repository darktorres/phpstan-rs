<?php
            class Payload {}
            function f() {
                $cb = function (Payload $x): Payload { return $x; };
            }
        