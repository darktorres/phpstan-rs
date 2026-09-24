<?php
            class MissingReturn {
                public string $m {
                    get {
                        if (true) {
                            $x = $this->m;
                        }
                    }
                }
            }
        