<?php
            class C {
                function regular(): bool {
                    return true;
                }
                function f(string $req): string {
                    $pos = strrpos($req, '.');
                    $pos !== false || $this->regular();
                    return substr($req, $pos);
                }
            }
        