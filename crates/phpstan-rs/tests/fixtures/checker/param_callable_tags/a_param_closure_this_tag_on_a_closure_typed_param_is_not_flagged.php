<?php
            class Foo {}
            /** @param-closure-this Foo $cb */
            function f(Closure $cb): void {}
        