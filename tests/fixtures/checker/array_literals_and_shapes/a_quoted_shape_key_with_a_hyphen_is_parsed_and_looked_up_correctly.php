<?php
            /** @param array{'content-type': string} $x */
            function f(array $x): void {
                $y = $x['content-type'];
                $z = $x['nonexistent-key'];
            }
        