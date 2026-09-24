<?php
            /** @param numeric-string $s */
            function sink(string $s): void {}
            function f(\DateTimeImmutable $dt): void {
                sink($dt->format('His'));
            }
        