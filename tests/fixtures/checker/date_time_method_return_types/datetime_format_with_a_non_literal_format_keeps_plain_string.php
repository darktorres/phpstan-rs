<?php
            /** @param numeric-string $s */
            function sink(string $s): void {}
            function f(\DateTime $dt, string $fmt): void {
                sink($dt->format($fmt));
            }
        