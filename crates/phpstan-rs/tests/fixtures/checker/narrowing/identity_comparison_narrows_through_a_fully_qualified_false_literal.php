<?php
            function f(string $path): string {
                if (\false === $pos = strpos($path, '://')) {
                    return '';
                }
                return substr($path, 0, $pos);
            }
        