<?php
            interface HandlerInterface {}
            class Dispatcher { public function dispatch(string $e): void {} }
            class MyHandler implements HandlerInterface {
                public function f(Dispatcher $d): void { $g = $d->dispatch(...); }
            }
        