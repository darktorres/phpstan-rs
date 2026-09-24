<?php
            /**
             * @template-contravariant T
             */
            class Bad {
                /** @return T */
                public function produce() { return null; }
            }
        