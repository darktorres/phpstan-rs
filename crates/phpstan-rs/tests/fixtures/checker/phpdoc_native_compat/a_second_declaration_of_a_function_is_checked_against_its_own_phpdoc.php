<?php
            if (PHP_VERSION_ID > 80000) {
                function twice($a) { return 1; }
            } else {
                /** @param array $a */
                function twice($a) { return 1; }
            }
        