<?php
            class NotAThrowable {}
            class Widget {
                public int $x {
                    /** @throws NotAThrowable */
                    get {
                        return 1;
                    }
                }
            }
        