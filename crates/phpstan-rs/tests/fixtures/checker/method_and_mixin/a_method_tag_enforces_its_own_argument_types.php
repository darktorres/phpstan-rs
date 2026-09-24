<?php
            /** @method void setX(int $x) */
            class Point {}
            function test(Point $p): void {
                $p->setX("not an int");
            }
        