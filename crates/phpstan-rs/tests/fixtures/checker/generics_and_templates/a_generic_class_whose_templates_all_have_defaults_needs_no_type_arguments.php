<?php
            /** @template T = mixed */
            final class Box {}
            /**
             * @template K
             * @template V = int
             */
            final class Pair {}
            final class Holder {
                public function box(Box $b): void {}
                public function pair(Pair $p): void {}
            }
        