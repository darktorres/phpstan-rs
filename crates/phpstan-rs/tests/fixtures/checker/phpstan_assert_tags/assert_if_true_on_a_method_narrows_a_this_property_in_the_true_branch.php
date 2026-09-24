<?php
            class Foo {
                private ?string $testFilesFile = null;

                /** @phpstan-assert-if-true !null $this->testFilesFile */
                public function hasTestFilesFile(): bool {
                    return $this->testFilesFile !== null;
                }

                public function testFilesFile(): string {
                    if (!$this->hasTestFilesFile()) {
                        throw new \RuntimeException('none');
                    }
                    return $this->testFilesFile;
                }
            }
        