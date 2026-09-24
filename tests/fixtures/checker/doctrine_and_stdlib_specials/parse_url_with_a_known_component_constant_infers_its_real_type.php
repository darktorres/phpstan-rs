<?php
            /**
             * @return int<0, 65535>|false|null
             */
            function f(string $url) {
                return parse_url($url, PHP_URL_PORT);
            }
        