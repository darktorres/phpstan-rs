<?php
            /** @param array{key: int} $x */
            function test(array $x): void {
                $x['key'] ??= 0;
            }
        