<?php
            function f(?string $filename): void {
                $handle = $filename !== null ? @fopen($filename, 'wb') : false;
                if ($handle !== false) {
                    fclose($handle);
                }
            }
        