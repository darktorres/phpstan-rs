<?php
            /**
             * @template-covariant T
             */
            class Box {
                /**
                 * @var T
                 */
                public mixed $value {
                    get => $this->value;
                }
            }
        