<?php
            final class Foo {}
            final class Bar {}
            function f(): Foo&Bar { throw new \Exception(); }
        