<?php
            abstract class Widget {
                abstract public int $x {
                    /** @throws \RuntimeException */
                    get;
                }
            }
        