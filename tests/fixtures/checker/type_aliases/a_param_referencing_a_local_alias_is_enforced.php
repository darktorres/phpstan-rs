<?php
            /**
             * @phpstan-type Options array{verbose: bool}
             */
            class Runner {
                /**
                 * @param Options $options
                 */
                public function run(array $options): void {}
            }
            function run(Runner $r): void {
                $r->run(['verbose' => 'yes']);
            }
        