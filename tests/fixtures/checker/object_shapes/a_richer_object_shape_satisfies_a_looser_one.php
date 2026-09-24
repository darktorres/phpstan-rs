<?php
            /** @param object{x: int} $p */
            function sink($p): void {}
            /** @param object{x: int, y: int} $p */
            function f($p): void {
                sink($p);
            }
        