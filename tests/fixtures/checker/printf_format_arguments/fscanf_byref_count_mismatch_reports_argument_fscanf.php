<?php
            /** @param resource $stream */
            function f($stream) {
                $a = 0; $b = ''; $c = 0;
                fscanf($stream, "%d %s", $a, $b, $c);
            }
        