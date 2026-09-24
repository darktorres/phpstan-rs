<?php
            class Logger { public function log(string $msg): void {} }
            class Wrapper { public function f(Logger $l): void { $l->log('a'); } }
            class Other { public function f(Logger $l): void { $l->log('b'); } }
        