<?php
            class FullyVirtual {
                public string $name {
                    get => "constant";
                    set(string $value) {
                        echo $value;
                    }
                }
            }
        