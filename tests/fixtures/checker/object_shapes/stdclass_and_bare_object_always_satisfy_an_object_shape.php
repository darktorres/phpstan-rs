<?php
            /** @param object{x: int, y: int} $p */
            function sink($p): void {}
            function f(stdClass $s, object $o): void {
                sink($s);
                sink($o);
            }
        