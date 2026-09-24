<?php
            /**
             * @param string $separator
             * @param string $string
             * @return string[]|false
             */
            #[LanguageLevelTypeAware(["8.0" => "string[]"], default: "string[]|false")]
            function explode($separator, $string) {}
            function f(string $s): array {
                return explode(',', $s);
            }
        