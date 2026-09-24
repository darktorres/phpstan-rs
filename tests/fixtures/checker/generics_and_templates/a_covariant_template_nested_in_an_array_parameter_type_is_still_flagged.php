<?php
            /**
             * @template-covariant T
             */
            class Bad {
                /** @param T[] $items */
                public function consumeAll($items): void {}
            }
        