<?php
            class Proxy {
                public static function __callStatic($name, $args) { return null; }
            }
            function f() {
                Proxy::anything();
            }
        