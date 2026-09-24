<?php
            /** @param non-empty-string $s */
            function sink(string $s): void {}
            function f(\DateInterval $i): void {
                sink($i->format('%d'));
            }
        