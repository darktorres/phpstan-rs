<?php
            class Base {}
            class Variable extends Base { public string $name = ""; }
            class Arg { public Base $value; }
            function f(Arg $arg) {
                if ($arg->value instanceof Variable && $arg->value->name !== '') {
                    echo 'narrowed';
                } else {
                    echo $arg->value->name;
                }
            }
        