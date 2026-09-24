<?php
            class Event {}
            class Other {}
            class Dispatcher {
                /**
                 * @template T of Event
                 * @param callable(T): void $handler
                 */
                public function on(callable $handler): void {}
            }
            function f(Dispatcher $d): void {
                $d->on(function (Other $e): void {});
            }
        