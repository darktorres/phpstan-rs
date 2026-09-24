<?php
            class Base {
                public static int $x = 1;
            }
            function f(): void {
                $y = base::$x;
            }
        