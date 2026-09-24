<?php
            class Foo {
                private ?string $x = null;

                /** @phpstan-assert-if-false !null $this->x */
                public function isXNull(): bool {
                    return $this->x === null;
                }

                public function needsX(): string {
                    if ($this->isXNull()) {
                        throw new \RuntimeException('none');
                    }
                    return $this->x;
                }
            }
        