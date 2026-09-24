<?php
            class Event {}
            class Dispatcher {
                /**
                 * @template T of object
                 * @param class-string<T> $class
                 * @param callable(T): (T|void) $handler
                 */
                public function addTypedHandler(string $class, callable $handler): void {}
            }
            function f(Dispatcher $d): void {
                $d->addTypedHandler(Event::class, function (Event $e): Event { return $e; });
                $d->addTypedHandler(Event::class, function (Event $e): void {});
            }
        