<?php
            class Types {}

            /**
             * @phpstan-import-type Nonexistent from Types
             */
            class ErrorBag {
                /**
                 * @return Nonexistent
                 */
                public function firstError(): array {
                    return [];
                }
            }
        