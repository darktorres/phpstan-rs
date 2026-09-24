<?php
            class Base { public int $x = 1; }
            class Child extends Base {
                #[\Override]
                public int $x = 2;
            }
        