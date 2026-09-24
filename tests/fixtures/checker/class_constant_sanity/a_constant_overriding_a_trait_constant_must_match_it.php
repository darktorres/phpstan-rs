<?php
            trait Inner { public const DEEP = 1; }
            trait Outer {
                use Inner;
                public const VIS = 1;
                final public const FIN = 1;
                public const int TYPED = 1;
                public const VALUE = 1;
                public const SAME = 'x';
            }
            class Host {
                use Outer;
                protected const VIS = 1;
                public const FIN = 1;
                public const TYPED = 1;
                public const VALUE = 2;
                public const SAME = 'x';
                final public const DEEP = 5;
            }
        