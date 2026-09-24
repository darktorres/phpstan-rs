<?php
            /**
             * @template U
             */
            class Collection {}
            /**
             * @template-covariant T
             */
            class Bad {
                /** @param Collection<T> $c */
                public function consume($c): void {}
            }
        