<?php
            /** @template TKey of array-key
             *  @template T */
            interface Coll {}
            /** @template TKey of array-key
             *  @template T
             *  @template-implements Coll<TKey, T> */
            class ArrColl implements Coll {
                /** @param array<TKey, T> $elements */
                public function __construct(array $elements = []) {}
            }
            class Holder {
                /** @var Coll<int, string> */
                public Coll $c;
                public function __construct() { $this->c = new ArrColl(); }
            }
        