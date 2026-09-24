<?php
            /**
             * @template-covariant T
             */
            class Bad {
                /** @param T $x */
                public function consume($x): void {}
            }
        