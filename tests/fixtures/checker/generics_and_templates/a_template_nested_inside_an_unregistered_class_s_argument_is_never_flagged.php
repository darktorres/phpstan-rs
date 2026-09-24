<?php
            /**
             * @template-covariant T
             */
            class Bad {
                /** @param Unregistered<T> $x */
                public function consume($x): void {}
            }
        