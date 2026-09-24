<?php
            class Hooked {
                public int $n { get => (fn (private int $x) => $x)(1); }
            }
        