<?php
            class NestedClosureWrite {
                public string $k {
                    get => $this->k;
                    set(string $value) {
                        (function () use ($value) {
                            $this->k = $value;
                        })();
                    }
                }
            }
        