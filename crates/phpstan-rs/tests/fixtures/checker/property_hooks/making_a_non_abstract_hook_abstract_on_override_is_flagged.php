<?php
            class Base {
                private int $backing = 0;
                public int $x {
                    get => $this->backing;
                }
            }
            abstract class Child extends Base {
                abstract public int $x { get; }
            }
        