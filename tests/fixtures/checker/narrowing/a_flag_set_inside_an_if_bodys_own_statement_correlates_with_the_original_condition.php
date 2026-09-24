<?php
            function needsInt(int $x): void {}
            function f(int|false $mtime, int $threshold): void {
                $need_generate = false;
                if ($mtime === false or $mtime < $threshold) {
                    $need_generate = true;
                }
                if (! $need_generate) {
                    needsInt($mtime);
                }
            }
        