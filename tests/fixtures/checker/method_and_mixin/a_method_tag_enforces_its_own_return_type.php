<?php
            /** @method int getX() */
            class Point {}
            function needsString(string $s): void {}
            function test(Point $p): void {
                needsString($p->getX());
            }
        