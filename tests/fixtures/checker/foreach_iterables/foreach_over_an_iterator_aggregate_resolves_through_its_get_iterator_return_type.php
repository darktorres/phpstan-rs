<?php
            use function PHPStan\Testing\assertType;
            /** @implements \Iterator<int, string> */
            class StringIterator implements \Iterator {
                private array $items = ['a', 'b'];
                private int $pos = 0;
                public function current(): mixed { return $this->items[$this->pos]; }
                public function next(): void { $this->pos++; }
                public function key(): mixed { return $this->pos; }
                public function valid(): bool { return isset($this->items[$this->pos]); }
                public function rewind(): void { $this->pos = 0; }
            }
            class StringCollection implements \IteratorAggregate {
                public function getIterator(): StringIterator {
                    return new StringIterator();
                }
            }
            function f(StringCollection $c): void {
                foreach ($c as $k => $v) {
                    assertType('int', $k);
                    assertType('string', $v);
                }
            }
        