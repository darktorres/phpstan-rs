<?php
                class Invokable {
                    function __invoke() {}
                }
                function f(Invokable $x) {
                    $x();
                }
            