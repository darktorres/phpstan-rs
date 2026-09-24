<?php
            class Point {
                public string $x = '';
            }
            /** @param object{x: int} $p */
            function sink($p): void {}
            function f(Point $p): void {
                sink($p);
            }
        