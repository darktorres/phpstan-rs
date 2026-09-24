<?php
            class Temperature {
                private float $celsius = 0.0;
                public float $fahrenheit {
                    get => $this->celsius * 9 / 5 + 32;
                    set(float $value) {
                        $this->celsius = ($value - 32) * 5 / 9;
                    }
                }
            }
        