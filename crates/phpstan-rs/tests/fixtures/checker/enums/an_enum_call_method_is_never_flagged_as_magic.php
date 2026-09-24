<?php
            enum Status {
                case Active;
                public function __call($name, $args) {}
                public static function __callStatic($name, $args) {}
                public function __invoke() {}
            }
        