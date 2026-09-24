<?php
            use function PHPStan\Testing\assertType;
            /** @implements \IteratorAggregate<int, string> */
            class StringCollection implements \IteratorAggregate {
                public function getIterator(): \Iterator {
                    throw new \RuntimeException('not implemented in this test');
                }
            }
            function f(StringCollection $c): void {
                foreach ($c as $k => $v) {
                    assertType('int', $k);
                    assertType('string', $v);
                }
            }
        