<?php
            class Foo {
                const void A = 1;
                const callable B = 'strlen';
                public void $a;
                public never $b;
                public function m(void $x, never $y, static $z): static { return $this; }
            }
            