<?php
            /** @method int getX() */
            class Point {}
            function test(Point $p): int {
                return $p->getX();
            }
        