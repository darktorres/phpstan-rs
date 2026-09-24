<?php
            /**
             * @template U
             */
            class Collection {}
            /**
             * @template-covariant T
             */
            class Good {
                /** @param Collection<contravariant T> $c */
                public function consume($c): void {}
            }
        