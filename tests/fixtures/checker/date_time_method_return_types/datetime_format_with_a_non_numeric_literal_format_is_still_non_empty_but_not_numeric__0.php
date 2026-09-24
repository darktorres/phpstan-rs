<?php
            /** @param non-empty-string $s */
            function sink(string $s): void {}
            function f(\DateTime $dt): void {
                sink($dt->format('lY'));
            }
        