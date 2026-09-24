<?php
            /**
             * @template T
             */
            class Collection {
                /** @return T */
                public function first() { return null; }
            }
            /** @extends Collection<int> */
            class IntCollection extends Collection {}
            /**
             * @param IntCollection $c
             * @return string
             */
            function f($c): string {
                return $c->first();
            }
        