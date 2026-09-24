<?php
                class Container implements ArrayAccess {
                    function offsetExists($offset): bool { return true; }
                    function offsetGet($offset): mixed { return null; }
                    function offsetSet($offset, $value): void {}
                    function offsetUnset($offset): void {}
                }
                function f(Container $c) {
                    [$a, $b] = $c;
                }
            