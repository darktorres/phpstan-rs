<?php
            function sink(object $o): void {}
            /** @param object{x: int} $p */
            function f($p): void {
                sink($p);
            }
        