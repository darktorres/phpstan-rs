<?php
            interface Node {}
            interface Rule {
                public function process(Node $node): void;
            }
            /**
             * @phpstan-type SpecificNode array{kind: string}
             */
            class MyRule implements Rule {
                /**
                 * @param SpecificNode $node
                 */
                public function process(Node $node): void {}
            }
        