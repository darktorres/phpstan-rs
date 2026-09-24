<?php
            class Logger { public function log(string $msg): void {} }
            class Foo {
                public function allowed(Logger $l): void { $l->log('a'); }
                public function other(Logger $l): void { $l->log('b'); }
            }
        