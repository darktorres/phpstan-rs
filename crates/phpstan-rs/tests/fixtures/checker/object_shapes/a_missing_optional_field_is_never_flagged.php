<?php
            class Point {
                public int $x = 0;
            }
            /** @param object{x: int, y?: int} $p */
            function sink($p): void {}
            function f(Point $p): void {
                sink($p);
            }
        