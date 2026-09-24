<?php
            use function PHPStan\Testing\assertType;
            class Handle {
                /** @var closed-resource|resource|null */
                private mixed $stream = null;
                public function close(): void {
                    if (is_resource($this->stream)) {
                        assertType('resource', $this->stream);
                    }
                }
                /** @param closed-resource $c */
                public function dead($c): void {
                    if (is_resource($c)) {}
                }
            }
        