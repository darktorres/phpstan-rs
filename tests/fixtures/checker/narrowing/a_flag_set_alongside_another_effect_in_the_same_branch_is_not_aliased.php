<?php
            function needsInt(int $x): void {}
            function sideEffect(): void {}
            function f(int|false $mtime): void {
                $need_generate = false;
                if ($mtime === false) {
                    $need_generate = true;
                    sideEffect();
                }
                if (! $need_generate) {
                    needsInt($mtime);
                }
            }
        