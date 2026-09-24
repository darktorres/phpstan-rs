<?php
            class Widget {
                public int $x {
                    /** @throws \RuntimeException */
                    get {
                        throw new \RuntimeException('documented');
                    }
                }
            }
        