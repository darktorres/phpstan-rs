<?php
            /**
             * @template-covariant V
             */
            class Producer {}
            /**
             * @template-covariant T
             */
            class Bad {
                /** @param Producer<T> $x */
                public function consume($x): void {}
            }
        