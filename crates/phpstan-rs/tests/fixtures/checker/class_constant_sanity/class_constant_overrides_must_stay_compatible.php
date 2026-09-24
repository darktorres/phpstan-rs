<?php
            interface HasLimit {
                const LIMIT = 10;
                final const FIXED = 1;
            }
            class Base {
                public const OPEN = 'a';
                protected const GUARDED = 'b';
                private const HIDDEN = 'c';
                const int TYPED = 1;
                /** @var positive-int */
                const DOC = 5;
            }
            class Child extends Base implements HasLimit {
                const LIMIT = 20;
                const FIXED = 2;
                protected const OPEN = 'x';
                private const GUARDED = 'y';
                private const HIDDEN = 'z';
                const TYPED = 2;
                /** @var int */
                const DOC = -1;
            }
            class TypedChild extends Base {
                const string TYPED = 'no';
            }
        