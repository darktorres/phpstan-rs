<?php
            /** @param non-falsy-string $s */
            function sink(string $s): void {}
            function f(\DateInterval $i): void {
                sink($i->format('%Y years'));
            }
        