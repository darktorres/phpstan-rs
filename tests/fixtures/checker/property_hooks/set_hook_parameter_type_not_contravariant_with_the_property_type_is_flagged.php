<?php
            class ContravarianceBad {
                public string $w {
                    set(int $value) {
                        echo $value;
                    }
                }
            }
        