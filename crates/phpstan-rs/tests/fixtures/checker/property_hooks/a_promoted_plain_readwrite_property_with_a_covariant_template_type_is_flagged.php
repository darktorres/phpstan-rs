<?php
            /**
             * @template-covariant T
             */
            class BoxPromotedMutable {
                /**
                 * @param T $value
                 */
                public function __construct(
                    public mixed $value,
                ) {}
            }
        