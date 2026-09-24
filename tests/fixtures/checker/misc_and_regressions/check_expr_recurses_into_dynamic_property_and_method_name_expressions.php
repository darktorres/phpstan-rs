<?php
            function f($obj) {
                $obj->{$undefinedInDynPropBrace};
                $obj->$undefinedInDynPropSingle;
                $obj->$$undefinedInDynMethodDollarDollar();
            }
        