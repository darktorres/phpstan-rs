<?php
            class Widget {
                public int $x {
                    /** @throws void */
                    set(int $value) {
                        throw new \RuntimeException('surprise');
                    }
                }
            }
        