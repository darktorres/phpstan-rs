<?php
            /** @param array<mixed> $vars */
            function f(array $vars) {
                sscanf("1 x", "%d %s", ...$vars);
            }
        