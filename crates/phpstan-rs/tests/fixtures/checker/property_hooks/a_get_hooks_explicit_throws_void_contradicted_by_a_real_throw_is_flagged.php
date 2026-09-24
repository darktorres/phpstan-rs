<?php
            class Widget {
                public int $x {
                    /** @throws void */
                    get {
                        throw new \RuntimeException('surprise');
                    }
                }
            }
        