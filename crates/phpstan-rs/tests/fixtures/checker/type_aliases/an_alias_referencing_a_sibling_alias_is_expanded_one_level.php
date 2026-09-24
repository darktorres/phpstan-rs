<?php
            /**
             * @phpstan-type Error array{code: int, message: string}
             * @phpstan-type ErrorList list<Error>
             */
            class ErrorBag {
                /**
                 * @return ErrorList
                 */
                public function errors(): array {
                    return [];
                }
            }
            function needsInt(int $x): void {}
            function run(ErrorBag $bag): void {
                foreach ($bag->errors() as $error) {
                    needsInt($error['code']);
                }
            }
        