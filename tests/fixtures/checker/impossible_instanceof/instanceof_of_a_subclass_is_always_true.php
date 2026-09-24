<?php
            interface Shape {}
            class Square implements Shape {}
            function f(Square $s): void {
                if ($s instanceof Shape) { echo 1; }
            }
        