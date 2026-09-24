<?php
            /** @method string getX() */
            class Point {
                public function getX(): int { return 1; }
            }
            function needsInt(int $x): void {}
            function test(Point $p): void {
                needsInt($p->getX());
            }
        