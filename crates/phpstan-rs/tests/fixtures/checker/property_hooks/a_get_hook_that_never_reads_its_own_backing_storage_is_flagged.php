<?php
            class BadGet {
                public string $y {
                    get => "constant";
                    set(string $value) {
                        $this->y = $value;
                    }
                }
            }
        