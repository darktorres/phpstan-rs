<?php
            class WiderParamOk {
                public int $w {
                    set(int|string $value) {
                        echo $value;
                    }
                }
            }
        