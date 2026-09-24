<?php
            /** @param numeric-string $s */
            function sink(string $s): void {}
            function f(\DateTime $dt): void {
                sink($dt->format('Ymd'));
            }
        