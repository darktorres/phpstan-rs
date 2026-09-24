<?php
            function f(string $path) {
                if (!is_dir($path) || !$dir = opendir($path)) {
                    die();
                }
                readdir($dir);
            }
        