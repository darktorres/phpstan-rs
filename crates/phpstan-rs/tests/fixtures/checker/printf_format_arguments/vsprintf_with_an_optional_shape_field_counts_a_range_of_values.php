<?php
            /** @param array{0: string, 1?: int, 2: string} $args */
            function f(array $args) {
                vsprintf('%s %d %s', $args);
            }
        