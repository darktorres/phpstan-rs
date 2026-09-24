<?php
        class Gated {
            final public int $a = 1;
            public int $b { get => 1; }
            public static private(set) int $c = 1;
        }
    