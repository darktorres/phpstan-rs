<?php
            class EventDispatcher {
                /**
                 * @template T of object
                 * @param T $event
                 * @return T
                 */
                public function dispatch(object $event): object {
                    return $event;
                }
            }
            final class RenderTagName {
                public function __construct(public string $tagName) {}
            }
            function needsString(string $s): void {}
            function f(EventDispatcher $d): void {
                $result = $d->dispatch(new RenderTagName('x'));
                needsString($result->tagName);
            }
        