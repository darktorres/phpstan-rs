<?php
            /** @return array{0: resource, 1: array<int, resource>} */
            function spawn(array $cmd, array $descriptors): array {
                $proc = proc_open($cmd, $descriptors, $pipes);
                if ($proc === false) {
                    throw new Exception('proc_open failed');
                }
                return [$proc, $pipes];
            }
        