<?php
            class Translator {
                /** @var array<string, string|array<int, string>> */
                private array $mirror = [];

                public function translate(string $key): string {
                    $val = $key;
                    if (is_string($this->mirror[$key])) {
                        $val = $this->mirror[$key];
                    }
                    return $val;
                }
            }
        