<?php
            interface HandlerInterface {}
            class Dispatcher {
                public function dispatch(): void {}
            }
            class MyHandler implements HandlerInterface {
                public function f(Dispatcher $d): void { $d->dispatch(); }
            }
            class Other {
                public function f(Dispatcher $d): void { $d->dispatch(); }
            }
        