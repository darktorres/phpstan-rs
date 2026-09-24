<?php
            class C {
                function ierror(string $msg): never {
                    throw new Exception($msg);
                }
                function f(string $req): string {
                    $pos = strrpos($req, '.');
                    $pos !== false || $this->ierror('Missing .');
                    return substr($req, $pos);
                }
            }
        