<?php
            function merge(array $config, array $defaults): int {
                $merged = $config + $defaults;
                return count($merged);
            }
        