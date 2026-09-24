<?php
            class Foo {}
            function f(Foo $x): string { return $x->missing ?? "default"; }
        