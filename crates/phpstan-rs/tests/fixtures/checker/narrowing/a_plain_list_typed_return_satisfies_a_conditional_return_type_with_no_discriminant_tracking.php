<?php
            /** @return ($flag is true ? list<string> : list<int>) */
            function f(bool $flag): array {
                return ['a', 'b'];
            }
        