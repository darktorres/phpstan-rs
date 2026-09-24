<?php
            class Base {
                private int $backing = 0;
                public int $x {
                    get => $this->backing;
                    set(int $v) { $this->backing = $v; }
                }
            }
            class Child extends Base {
                public int $x {
                    get => 5;
                }
            }
        