<?php
            class Foo {
                /** @deprecated use NEW_CONST instead */
                public const OLD_CONST = 1;
            }
            function f(): int {
                return Foo::OLD_CONST;
            }
        