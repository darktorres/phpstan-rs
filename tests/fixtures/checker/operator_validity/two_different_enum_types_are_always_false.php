<?php
            enum Status { case A; }
            enum Color { case Red; }
            function f(Status $s, Color $c): void {
                if ($s === $c) { echo 1; }
            }
        