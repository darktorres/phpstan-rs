<?php
            /**
             * @template-covariant V
             */
            class Producer {}
            /**
             * @template-covariant T
             */
            class Good {
                /** @return Producer<T> */
                public function produce() { return new Producer(); }
            }
        