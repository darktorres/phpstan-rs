<?php
            /**
             * @template-contravariant V
             */
            class Sink {}
            /**
             * @template-contravariant T
             */
            class Good {
                /** @return Sink<T> */
                public function makeSink() { return new Sink(); }
            }
        