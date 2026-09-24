<?php
            /**
             * @param string $string
             * @param int $offset
             * @param ?int $length
             * @return string|false
             */
            #[LanguageLevelTypeAware(["8.0" => "string"], default: "string|false")]
            function substr($string, $offset, $length) {}
            function f(string $s): string {
                return substr($s, 0, -1);
            }
        