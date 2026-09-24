<?php
            class Payload {}
            function f(Payload $p) {
                $cb = fn (): TotallyMadeUpArrowReturn => $p;
            }
        