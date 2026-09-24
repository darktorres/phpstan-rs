<?php
            class Widget {
                public int $x {
                    set(int $value) {
                        throw new \RuntimeException('undocumented');
                    }
                }
            }
        