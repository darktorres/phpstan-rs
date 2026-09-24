<?php
            class Holder {
                /** @phpstan-import-type TestIndexType from Holder */
                /** @var array<TestIndexType, int> */
                private array $hit = [];

                /** @param TestIndexType $testIndex */
                public function recordHit($testIndex): void {
                    $this->hit[$testIndex] = 1;
                }
            }
        