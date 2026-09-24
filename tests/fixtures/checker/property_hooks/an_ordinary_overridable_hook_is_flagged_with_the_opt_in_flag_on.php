<?php
            class Widget {
                public int $x {
                    /** @throws \RuntimeException */
                    get {
                        return 1;
                    }
                }
            }
        