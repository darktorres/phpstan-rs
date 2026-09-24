<?php
            class Logger { public function log(string $msg): void {} }
            class Foo {
                public function other(Logger $l): void { $f = $l->log(...); }
            }
        