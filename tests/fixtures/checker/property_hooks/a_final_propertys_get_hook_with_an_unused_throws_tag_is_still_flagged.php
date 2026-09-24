<?php
            class Widget {
                final public int $x {
                    /** @throws \RuntimeException */
                    get {
                        return 1;
                    }
                }
            }
        