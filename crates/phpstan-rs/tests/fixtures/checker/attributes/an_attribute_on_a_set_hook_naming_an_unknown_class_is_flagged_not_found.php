<?php
            class C {
                public int $x {
                    #[NoSuchAttr]
                    set(int $v) { $this->x = $v; }
                }
            }
        