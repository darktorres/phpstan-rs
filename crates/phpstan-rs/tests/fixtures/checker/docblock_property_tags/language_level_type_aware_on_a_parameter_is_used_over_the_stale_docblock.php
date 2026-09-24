<?php
            class finfo {}
            /** @return finfo|false */
            #[LanguageLevelTypeAware(["8.1" => "finfo|false"], default: "resource|false")]
            function finfo_open() {}
            /**
             * @param resource $finfo
             * @return string|false
             */
            function finfo_file(#[LanguageLevelTypeAware(['8.1' => 'finfo'], default: 'resource')] $finfo, string $filename) {}
            function f(string $path): string {
                $handle = finfo_open();
                if ($handle === false) {
                    return '';
                }
                $result = finfo_file($handle, $path);
                return $result === false ? '' : $result;
            }
        