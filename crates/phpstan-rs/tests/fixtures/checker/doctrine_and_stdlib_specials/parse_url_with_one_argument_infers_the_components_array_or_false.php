<?php
            /**
             * @return array{scheme?: string, host?: string, port?: int<0, 65535>, user?: string, pass?: string, path?: string, query?: string, fragment?: string}|false
             */
            function f(string $url) {
                return parse_url($url);
            }
        