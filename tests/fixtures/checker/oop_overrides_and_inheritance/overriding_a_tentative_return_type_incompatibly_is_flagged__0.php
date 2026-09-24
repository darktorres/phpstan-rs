<?php
        class TentIt implements IteratorAggregate {
            public function getIterator() { return new ArrayIterator([]); }
        }
        class TentCount implements Countable {
            public function count(): string { return ''; }
        }
        class TentOk implements Countable {
            public function count(): int { return 0; }
        }
        class TentWillChange implements JsonSerializable {
            #[\ReturnTypeWillChange]
            public function jsonSerialize() { return 1; }
        }
        class TentMixed implements ArrayAccess {
            public function offsetExists($o): bool { return true; }
            public function offsetGet($o) { return 1; }
            public function offsetSet($o, $v): void {}
            public function offsetUnset($o): void {}
        }
    