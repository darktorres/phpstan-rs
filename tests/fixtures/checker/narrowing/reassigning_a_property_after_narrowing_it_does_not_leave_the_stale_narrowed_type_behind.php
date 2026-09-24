<?php
            class Cache {
                /** @var array<string>|null */
                private ?array $entries = null;

                /** @return array<string> */
                private function entries(): array {
                    if ($this->entries === null) {
                        $this->entries = ['a', 'b'];
                    }
                    return $this->entries;
                }

                /** @return array<string> */
                public function get(): array {
                    return $this->entries();
                }
            }
        