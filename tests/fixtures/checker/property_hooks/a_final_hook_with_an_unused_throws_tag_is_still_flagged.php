<?php
            class Widget {
                public int $x {
                    /** @throws \RuntimeException */
                    final get {
                        return 1;
                    }
                }
            }
        