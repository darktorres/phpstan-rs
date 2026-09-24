<?php
            use function PHPStan\Testing\assertType;
            class UntypedIterator implements \Iterator {
                public function current(): mixed { return null; }
                public function next(): void {}
                public function key(): mixed { return null; }
                public function valid(): bool { return false; }
                public function rewind(): void {}
            }
            function f(UntypedIterator $it): void {
                foreach ($it as $k => $v) {
                    assertType('mixed', $k);
                    assertType('mixed', $v);
                }
            }
        