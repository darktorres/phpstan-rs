<?php
            class Holder {
                public ?string $captured = null;

                /** @param list<string> $collected */
                public function run(array $collected): void {
                    $this->captured = $collected === [] ? null : preg_replace('/^x/', '', $collected[0]);
                }
            }
        