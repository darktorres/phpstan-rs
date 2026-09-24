<?php
            /**
             * @template-contravariant T
             */
            class Consumer {
                /** @param T $x */
                public function consume($x): void {}
            }
        