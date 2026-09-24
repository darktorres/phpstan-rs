<?php
            class Standalone {
                public function f(): parent {
                    return $this;
                }
            }
        