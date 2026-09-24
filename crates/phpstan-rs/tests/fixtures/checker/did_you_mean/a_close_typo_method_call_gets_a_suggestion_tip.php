<?php
            class Point {
                public function getX(): int { return 0; }
            }
            function test(Point $p): int {
                return $p->getx_();
            }
        