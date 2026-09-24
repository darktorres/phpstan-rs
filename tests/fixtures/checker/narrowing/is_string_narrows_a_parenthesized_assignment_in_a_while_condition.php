<?php
            function readdir_(): string|false { return 'x'; }
            function f(): void {
                while (($file = readdir_()) !== false) {
                    preg_match('/x/', $file);
                }
            }
        