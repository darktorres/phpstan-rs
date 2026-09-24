<?php
            /**
             * @template-contravariant V
             */
            class Sink {}
            /**
             * @template-covariant T
             */
            class Bad {
                /** @return Sink<T> */
                public function makeSink() { return null; }
            }
        