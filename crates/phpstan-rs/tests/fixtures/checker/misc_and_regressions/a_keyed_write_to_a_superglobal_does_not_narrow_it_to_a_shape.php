<?php
            function f() {
                $_SERVER['SCRIPT_FILENAME'] = '/index.php';
                return $_SERVER['PHP_SELF'];
            }
        