<?php
            namespace App\Source;

            /** @phpstan-type Error array{code: int, message: string} */
            class Types {}

            namespace App\Consumer;

            use App\Source\Types;

            /** @phpstan-import-type Error from Types as ImportedError */
            class ErrorBag {
                /** @return ImportedError */
                public function firstError(): array {
                    return ['code' => 1, 'message' => 'oops'];
                }
            }
            function run(ErrorBag $bag): void {
                $error = $bag->firstError();
                $missing = $error['nope'];
            }
        