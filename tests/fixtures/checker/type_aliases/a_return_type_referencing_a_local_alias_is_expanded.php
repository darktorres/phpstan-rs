<?php
            /**
             * @phpstan-type Error array{code: int, message: string}
             */
            class ErrorBag {
                /**
                 * @return Error
                 */
                public function firstError(): array {
                    return ['code' => 1, 'message' => 'oops'];
                }
            }
            function run(ErrorBag $bag): void {
                $error = $bag->firstError();
                $code = $error['code'];
                $message = $error['message'];
                $missing = $error['nope'];
            }
        