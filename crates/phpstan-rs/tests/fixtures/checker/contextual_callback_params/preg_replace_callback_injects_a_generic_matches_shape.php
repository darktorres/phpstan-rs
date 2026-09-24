<?php
            function needsInt(int $i): void {}
            function f(string $s): string|null {
                return preg_replace_callback('/x/', function ($matches) {
                    needsInt($matches);
                    return $matches[0];
                }, $s);
            }
        