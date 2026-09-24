<?php
            class SameClassPrivateAccess {
                private float $celsius = 0.0;
                public float $fahrenheit {
                    get => $this->celsius;
                    set(float $value) {
                        $this->celsius = $value;
                    }
                }
            }
        