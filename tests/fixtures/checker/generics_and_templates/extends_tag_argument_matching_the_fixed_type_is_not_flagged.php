<?php
            /**
             * @template T
             */
            class Collection {
                /** @param T $item */
                public function add($item): void {}
            }
            /** @extends Collection<int> */
            class IntCollection extends Collection {}
            function f(IntCollection $c) {
                $c->add(1);
            }
        