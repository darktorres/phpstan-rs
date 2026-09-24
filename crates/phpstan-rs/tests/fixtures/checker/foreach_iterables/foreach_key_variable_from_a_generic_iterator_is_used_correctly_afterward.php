<?php
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
            function f(StringIterator $it): void {
                foreach ($it as $k => $v) {
                    $sum = $k + 1;
                }
            }
        