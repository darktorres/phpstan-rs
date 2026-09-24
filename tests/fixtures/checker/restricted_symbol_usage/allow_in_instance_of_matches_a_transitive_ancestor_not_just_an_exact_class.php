<?php
            class Logger { public function log(string $msg): void {} }
            interface Handler {}
            class BaseHandler implements Handler {
                public function f(Logger $l): void { $l->log('a'); }
            }
            class ConcreteHandler extends BaseHandler {}
        