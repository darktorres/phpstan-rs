<?php
            /** @method getX() */
            class Point {}
            function needsInt(int $x): void {}
            function test(Point $p): void {
                needsInt($p->getX());
            }
        