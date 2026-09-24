<?php
            /**
             * @template-covariant T
             */
            class BoxPromoted {
                /**
                 * @param T $value
                 */
                public function __construct(
                    public readonly mixed $value,
                ) {}
            }
        