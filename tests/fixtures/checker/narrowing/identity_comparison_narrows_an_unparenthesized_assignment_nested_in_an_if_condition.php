<?php
            function f(string $path): string {
                if (false !== $pos = strpos($path, '://')) {
                    return substr($path, 0, $pos + 3);
                }
                return '';
            }
        