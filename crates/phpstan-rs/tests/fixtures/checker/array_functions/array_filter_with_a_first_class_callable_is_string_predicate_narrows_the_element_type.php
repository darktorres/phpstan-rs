<?php
            /** @param array<mixed> $x
             *  @return array<string>
             */
            function f(array $x): array {
                return array_filter($x, is_string(...));
            }
        