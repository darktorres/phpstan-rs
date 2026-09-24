<?php
            class Base {
                private int $backing = 0;
                public int $x {
                    get => $this->backing;
                    set(int $v) { $this->backing = $v; }
                }
            }
            class Child extends Base {
                private int $backing2 = 0;
                public int $x {
                    get => $this->backing2;
                    set(int $v) { $this->backing2 = $v; }
                }
            }
        