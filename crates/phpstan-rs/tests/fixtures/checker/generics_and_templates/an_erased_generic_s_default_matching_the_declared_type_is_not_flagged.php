<?php
            /**
             * @template T = int
             */
            class Box {
                /** @return T */
                public function get() { return null; }
            }
            function f(): int {
                $b = new Box();
                return $b->get();
            }
        