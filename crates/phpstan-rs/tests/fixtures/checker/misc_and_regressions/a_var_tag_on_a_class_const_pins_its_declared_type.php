<?php
            final class Registry {
                /**
                 * @var list<string>
                 */
                public const array NAMES = ['a', 'b'];
            }
            /** @param list<string> $x */
            function wantsList(array $x): void {}
            wantsList(Registry::NAMES);
        