<?php
            function f($x) {
                if ($x instanceof TotallyMadeUpType) {
                    return true;
                }
                return false;
            }
        