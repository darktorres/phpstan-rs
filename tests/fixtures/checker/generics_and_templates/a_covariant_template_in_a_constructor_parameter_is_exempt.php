<?php
            /**
             * @template-covariant T
             */
            final class Success {
                /** @param T $value */
                public function __construct(private $value) {}
                /** @return T */
                public function get() {
                    return $this->value;
                }
            }
        