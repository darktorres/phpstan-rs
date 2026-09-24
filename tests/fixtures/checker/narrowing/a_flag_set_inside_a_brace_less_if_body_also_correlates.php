<?php
            function needsInt(int $x): void {}
            function f(int|false $mtime): void {
                $need_generate = false;
                if ($mtime === false) $need_generate = true;
                if (! $need_generate) {
                    needsInt($mtime);
                }
            }
        