<?php
            class RealType {}
            function f($x) {
                if ($x instanceof RealType) {
                    return true;
                }
                return false;
            }
        